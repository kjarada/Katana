#pragma once

// Every way apply(), rollback and recover() touch the disk, behind one
// interface. Two reasons it is an interface and not calls to
// std::filesystem:
//   1. The fault-injection loops (fault_fs.hpp) wrap it to crash the apply
//      before the k-th change and then require recover() to leave a tree
//      whose whole hash equals the old version's or the new one's - for
//      every k of a 40-file apply. That is only possible if every change
//      goes through one seam.
//   2. A rename of a file in use fails on Windows (errors 5, 32, 33: access
//      denied, sharing and lock violations, which antivirus scanners cause
//      too). The retry policy belongs to the real implementation, once.
//
// Paths are absolute and already resolved by the caller. An operation that
// fails returns a core::Error whose context begins `os_error=<n>` (the
// GetLastError or errno value, 0 when there was none), so a caller and a test
// can tell a locked file (32) from a missing one without parsing prose.
//
// The MUTATING operations - the ones FaultFs counts and may crash before - are
// exactly: makeDirectories, copyFile, renameFile, removeFile,
// removeEmptyDirectory, appendDurable, writeDurable, setExecutable. The rest
// only look.

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::update {

enum class EntryKind { Missing, File, Directory, Other };

struct EntryInfo {
    EntryKind kind = EntryKind::Missing;
    // True for a Windows reparse point (junction, symlink, mount point) and a
    // POSIX symlink. The apply refuses to write through one: a parent that is
    // a reparse point could send a write outside the install root.
    bool reparsePoint = false;
    std::uint64_t size = 0; // bytes, for a File
};

// What renameFile does when Windows says 5, 32 or 33: try again every
// `interval` for up to `total`, then fail with the last error. Never
// MOVEFILE_DELAY_UNTIL_REBOOT: a swap that completes at a reboot is a swap
// nobody can roll back.
inline constexpr int kRetryableOsErrors[] = {5, 32, 33};
struct RetryPolicy {
    std::chrono::milliseconds interval{250};
    std::chrono::milliseconds total{30'000};
};

// Holds the update lock while it lives (FsOps::tryLock).
class FileLock {
  public:
    virtual ~FileLock() = default;
};

class FsOps {
  public:
    virtual ~FsOps() = default;

    // ---- looking ----------------------------------------------------------

    // Never follows a link or reparse point: it describes the entry itself.
    // Missing is not an error.
    [[nodiscard]] virtual core::Result<EntryInfo> stat(const std::filesystem::path& path) const = 0;

    // The names directly inside `directory`, full paths, unsorted.
    [[nodiscard]] virtual core::Result<std::vector<std::filesystem::path>>
    listDirectory(const std::filesystem::path& directory) const = 0;

    // The whole file.
    [[nodiscard]] virtual core::Result<std::string>
    readFile(const std::filesystem::path& path) const = 0;

    // The path with every link resolved, existing prefix or not
    // (std::filesystem::weakly_canonical). For "does it still resolve under
    // the root".
    [[nodiscard]] virtual core::Result<std::filesystem::path>
    resolve(const std::filesystem::path& path) const = 0;

    // Bytes free to an unprivileged user on the volume holding `path`.
    [[nodiscard]] virtual core::Result<std::uint64_t>
    freeSpace(const std::filesystem::path& path) const = 0;

    // ---- changing (counted by FaultFs) -------------------------------------

    // The directory and any missing parents; success when it exists.
    [[nodiscard]] virtual core::Status makeDirectories(const std::filesystem::path& directory) = 0;

    // A new file `to` with the bytes of `from`, flushed to disk. Fails
    // (AlreadyExists) when `to` exists: the apply copies into a fresh folder
    // and must never overwrite by accident.
    [[nodiscard]] virtual core::Status copyFile(const std::filesystem::path& from,
                                                const std::filesystem::path& to) = 0;

    // Moves `from` to `to` on one volume, atomically. Fails (AlreadyExists)
    // when `to` exists; never replaces. Retries as RetryPolicy says.
    [[nodiscard]] virtual core::Status renameFile(const std::filesystem::path& from,
                                                  const std::filesystem::path& to) = 0;

    // Deletes a file; success when it is already gone.
    [[nodiscard]] virtual core::Status removeFile(const std::filesystem::path& path) = 0;

    // Deletes a directory that is empty; success when it is already gone;
    // fails (InvalidState) when it holds anything.
    [[nodiscard]] virtual core::Status removeEmptyDirectory(const std::filesystem::path& path) = 0;

    // Appends and flushes to stable storage before returning (FlushFileBuffers,
    // fsync): the journal's group commit. Creates the file when absent.
    [[nodiscard]] virtual core::Status appendDurable(const std::filesystem::path& path,
                                                     std::string_view bytes) = 0;

    // Writes the whole file (created or replaced) and flushes it: a record
    // other code reads back (installed.json, last.json, state.json).
    [[nodiscard]] virtual core::Status writeDurable(const std::filesystem::path& path,
                                                    std::string_view bytes) = 0;

    // Sets the execute permission bits where the platform has them (POSIX);
    // success doing nothing on Windows.
    [[nodiscard]] virtual core::Status setExecutable(const std::filesystem::path& path) = 0;

    // ---- durability and exclusion (not counted) ------------------------------

    // Flushes a directory's entries to stable storage after renames (POSIX
    // fsync of the directory); success doing nothing on Windows, where the
    // rename's own flush is the guarantee.
    [[nodiscard]] virtual core::Status syncDirectory(const std::filesystem::path& directory) = 0;

    // The update lock, R/.update/lock: created when absent and held until the
    // returned object is destroyed or the process ends (Windows: a handle
    // opened with share mode 0; POSIX: flock). InvalidState with the message
    // "held" when another process holds it; any other failure is the OS's.
    // Counted as nothing: an empty lock file changes no state worth crashing at.
    [[nodiscard]] virtual core::Result<std::unique_ptr<FileLock>>
    tryLock(const std::filesystem::path& path) = 0;
};

// The real file system. src/katana_update/fsops_win.cpp (T7) and
// fsops_posix.cpp (T18).
[[nodiscard]] std::unique_ptr<FsOps> makeSystemFsOps(const RetryPolicy& retry = {});

} // namespace katana::update
