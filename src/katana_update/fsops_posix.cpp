// The file system of include/katana/update/fsops.hpp on Linux and macOS, which is NOT WRITTEN
// YET. Task T18 writes it for Linux: rename(2) plus fsync of the directory, flock, symlink
// detection. On macOS it will only ever be compiled, never used to apply: a file changed inside
// Katana.app breaks the bundle's seal (docs/update.md). CMake compiles this file everywhere
// except Windows.
//
// Until it is written every operation REFUSES, with one Error, and touches nothing: no file, no
// folder, no lock. None answers "done" - a file system that said so of a rename it had not made
// would let a swap go on over a tree it never changed - and none answers "missing" or "empty"
// for a question it did not put to the disk.
//
// The CODE of the refusal, Unsupported, is part of what this file promises. The two places that
// would otherwise work through this file system and report its refusal as a failure of the disk
// ask it one question first, stat(), and on Unsupported say `reason=platform` instead:
//   stage.cpp                    the forms of stage() and preflight() that use the system's
//                                file system (they would say write_failed, or helper_missing)
//   ../katana_updater/main.cpp   the helper, before a swap or a recovery (apply() would say
//                                root_not_writable, and write a last.json that said so)
// A file system that works never answers stat() with Unsupported, so the day this file is
// written those two questions answer themselves and neither place needs an edit.

#include "katana/update/fsops.hpp"

#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::update {

namespace {

using Path = std::filesystem::path;

// What a person reads where a download or an install was asked for here. It names no task and
// no file: the window shows it as it is.
constexpr std::string_view kNotOnThisSystem =
    "Katana does not install updates itself on this system: it checks for a newer version, "
    "which is then downloaded from the releases page and installed by hand";

// `os_error=0` first, as fsops.hpp asks of every failure: no system call stands behind this one.
core::Error refusal(std::string_view operation)
{
    return core::makeError(core::ErrorCode::Unsupported, std::string(kNotOnThisSystem),
                           "os_error=0 FsOps::" + std::string(operation));
}

class NoFsOps final : public FsOps {
  public:
    [[nodiscard]] core::Result<EntryInfo> stat(const Path&) const override
    {
        return refusal("stat");
    }
    [[nodiscard]] core::Result<std::vector<Path>> listDirectory(const Path&) const override
    {
        return refusal("listDirectory");
    }
    [[nodiscard]] core::Result<std::string> readFile(const Path&) const override
    {
        return refusal("readFile");
    }
    [[nodiscard]] core::Result<Path> resolve(const Path&) const override
    {
        return refusal("resolve");
    }
    [[nodiscard]] core::Result<std::uint64_t> freeSpace(const Path&) const override
    {
        return refusal("freeSpace");
    }
    [[nodiscard]] core::Status makeDirectories(const Path&) override
    {
        return refusal("makeDirectories");
    }
    [[nodiscard]] core::Status copyFile(const Path&, const Path&) override
    {
        return refusal("copyFile");
    }
    [[nodiscard]] core::Status renameFile(const Path&, const Path&) override
    {
        return refusal("renameFile");
    }
    [[nodiscard]] core::Status removeFile(const Path&) override { return refusal("removeFile"); }
    [[nodiscard]] core::Status removeEmptyDirectory(const Path&) override
    {
        return refusal("removeEmptyDirectory");
    }
    [[nodiscard]] core::Status appendDurable(const Path&, std::string_view) override
    {
        return refusal("appendDurable");
    }
    [[nodiscard]] core::Status writeDurable(const Path&, std::string_view) override
    {
        return refusal("writeDurable");
    }
    [[nodiscard]] core::Status setExecutable(const Path&) override
    {
        return refusal("setExecutable");
    }
    [[nodiscard]] core::Status syncDirectory(const Path&) override
    {
        return refusal("syncDirectory");
    }
    [[nodiscard]] core::Result<std::unique_ptr<FileLock>> tryLock(const Path&) override
    {
        return refusal("tryLock");
    }
};

} // namespace

std::unique_ptr<FsOps> makeSystemFsOps(const RetryPolicy&)
{
    return std::make_unique<NoFsOps>();
}

} // namespace katana::update
