#pragma once

// A FsOps that wraps another and breaks on demand: the deterministic
// fault-injection seam of the update tests (docs/update.md).
//
//   crashBefore(k)      the k-th MUTATING operation (fsops.hpp lists them;
//                       counted from 1 across the wrapper's life) is not
//                       performed: SimulatedCrash is thrown in its place, as
//                       if the process had died between operation k-1 and k.
//                       What the earlier operations wrote stays on disk, which
//                       is the point: the test then runs recover() with a
//                       clean FsOps on that disk and requires a tree whose
//                       whole hash equals the old or the new version's.
//   failRenameWith(k,e) the k-th renameFile (counted among renames only)
//                       returns an error with context `os_error=<e>` (32 for a
//                       locked file) instead of performing the rename, as
//                       the real implementation does after its retries ran
//                       out; the apply must then roll back.
//
// A test sizes its loop by running the apply once with no fault and reading
// mutatingOperations(). The same wrapper goes round recover() to crash INSIDE
// a recovery (the second loop). katana_updater_test builds one from
// `--die-at N` (never in katana_updater: the option exists only in the test
// build, src/katana_updater/main.cpp).
//
// SimulatedCrash is deliberately not a core::Error: no code under test may
// catch it and carry on.

#include <cstdint>
#include <string>

#include "katana/update/fsops.hpp"

namespace katana::update {

struct SimulatedCrash {
    std::uint64_t operation = 0; // the 1-based index of the operation not performed
    std::string description;     // "renameFile R/bin/a.dll -> .update/t1/old/bin/a.dll"
};

class FaultFs final : public FsOps {
  public:
    // `inner` outlives this object.
    explicit FaultFs(FsOps& inner);

    void crashBefore(std::uint64_t operation);
    void failRenameWith(std::uint64_t renameIndex, int osError);
    // Clears both settings and the counters.
    void reset();

    // Mutating operations performed so far (not counting one that crashed).
    [[nodiscard]] std::uint64_t mutatingOperations() const;

    // FsOps: looking passes straight through; changing is counted and may
    // be made to crash or fail.
    [[nodiscard]] core::Result<EntryInfo> stat(const std::filesystem::path& path) const override;
    [[nodiscard]] core::Result<std::vector<std::filesystem::path>>
    listDirectory(const std::filesystem::path& directory) const override;
    [[nodiscard]] core::Result<std::string>
    readFile(const std::filesystem::path& path) const override;
    [[nodiscard]] core::Result<std::filesystem::path>
    resolve(const std::filesystem::path& path) const override;
    [[nodiscard]] core::Result<std::uint64_t>
    freeSpace(const std::filesystem::path& path) const override;
    [[nodiscard]] core::Status makeDirectories(const std::filesystem::path& directory) override;
    [[nodiscard]] core::Status copyFile(const std::filesystem::path& from,
                                        const std::filesystem::path& to) override;
    [[nodiscard]] core::Status renameFile(const std::filesystem::path& from,
                                          const std::filesystem::path& to) override;
    [[nodiscard]] core::Status removeFile(const std::filesystem::path& path) override;
    [[nodiscard]] core::Status removeEmptyDirectory(const std::filesystem::path& path) override;
    [[nodiscard]] core::Status appendDurable(const std::filesystem::path& path,
                                             std::string_view bytes) override;
    [[nodiscard]] core::Status writeDurable(const std::filesystem::path& path,
                                            std::string_view bytes) override;
    [[nodiscard]] core::Status setExecutable(const std::filesystem::path& path) override;
    [[nodiscard]] core::Status syncDirectory(const std::filesystem::path& directory) override;
    [[nodiscard]] core::Result<std::unique_ptr<FileLock>>
    tryLock(const std::filesystem::path& path) override;

  private:
    FsOps& inner_;
    std::uint64_t crashBefore_ = 0; // 0: never
    std::uint64_t failRename_ = 0;  // 0: never
    int failRenameError_ = 0;
    std::uint64_t operations_ = 0;
    std::uint64_t renames_ = 0;
};

} // namespace katana::update
