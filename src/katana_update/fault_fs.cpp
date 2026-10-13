// The fault-injecting FsOps of include/katana/update/fault_fs.hpp. It is in the library, not
// in a test, because katana_updater_test (--die-at N) and katana_update_e2e build one too;
// the linker takes this object only where something names FaultFs, so katana_updater, which
// does not, carries none of it.

#include "katana/update/fault_fs.hpp"

#include <utility>

#include "katana/core/path_text.hpp"

namespace katana::update {

namespace {

using Path = std::filesystem::path;

std::string text(const Path& path)
{
    return core::pathToUtf8(path);
}

} // namespace

FaultFs::FaultFs(FsOps& inner) : inner_(inner) {}

void FaultFs::crashBefore(std::uint64_t operation)
{
    crashBefore_ = operation;
}

void FaultFs::failRenameWith(std::uint64_t renameIndex, int osError)
{
    failRename_ = renameIndex;
    failRenameError_ = osError;
}

void FaultFs::reset()
{
    crashBefore_ = 0;
    failRename_ = 0;
    failRenameError_ = 0;
    operations_ = 0;
    renames_ = 0;
}

std::uint64_t FaultFs::mutatingOperations() const
{
    return operations_;
}

// Every changing operation goes through here first: the k-th is the one a crash replaces.
// The description is only built when the crash happens.
#define KATANA_FAULT_POINT(description)                                              \
    do {                                                                              \
        if (crashBefore_ != 0 && operations_ + 1 == crashBefore_) {                   \
            throw SimulatedCrash{operations_ + 1, (description)};                     \
        }                                                                             \
        ++operations_;                                                                \
    } while (false)

core::Result<EntryInfo> FaultFs::stat(const Path& path) const
{
    return inner_.stat(path);
}

core::Result<std::vector<Path>> FaultFs::listDirectory(const Path& directory) const
{
    return inner_.listDirectory(directory);
}

core::Result<std::string> FaultFs::readFile(const Path& path) const
{
    return inner_.readFile(path);
}

core::Result<Path> FaultFs::resolve(const Path& path) const
{
    return inner_.resolve(path);
}

core::Result<std::uint64_t> FaultFs::freeSpace(const Path& path) const
{
    return inner_.freeSpace(path);
}

core::Status FaultFs::makeDirectories(const Path& directory)
{
    KATANA_FAULT_POINT("makeDirectories " + text(directory));
    return inner_.makeDirectories(directory);
}

core::Status FaultFs::copyFile(const Path& from, const Path& to)
{
    KATANA_FAULT_POINT("copyFile " + text(from) + " -> " + text(to));
    return inner_.copyFile(from, to);
}

core::Status FaultFs::renameFile(const Path& from, const Path& to)
{
    KATANA_FAULT_POINT("renameFile " + text(from) + " -> " + text(to));
    ++renames_;
    if (failRename_ != 0 && renames_ == failRename_) {
        // What the real implementation reports once its retries have run out: the file was
        // not moved.
        return core::makeError(core::ErrorCode::FileExportFailure,
                               "the file could not be moved (injected)",
                               "os_error=" + std::to_string(failRenameError_) + " " + text(from) +
                                   " -> " + text(to));
    }
    return inner_.renameFile(from, to);
}

core::Status FaultFs::removeFile(const Path& path)
{
    KATANA_FAULT_POINT("removeFile " + text(path));
    return inner_.removeFile(path);
}

core::Status FaultFs::removeEmptyDirectory(const Path& path)
{
    KATANA_FAULT_POINT("removeEmptyDirectory " + text(path));
    return inner_.removeEmptyDirectory(path);
}

core::Status FaultFs::appendDurable(const Path& path, std::string_view bytes)
{
    KATANA_FAULT_POINT("appendDurable " + text(path));
    return inner_.appendDurable(path, bytes);
}

core::Status FaultFs::writeDurable(const Path& path, std::string_view bytes)
{
    KATANA_FAULT_POINT("writeDurable " + text(path));
    return inner_.writeDurable(path, bytes);
}

core::Status FaultFs::setExecutable(const Path& path)
{
    KATANA_FAULT_POINT("setExecutable " + text(path));
    return inner_.setExecutable(path);
}

core::Status FaultFs::syncDirectory(const Path& directory)
{
    return inner_.syncDirectory(directory);
}

core::Result<std::unique_ptr<FileLock>> FaultFs::tryLock(const Path& path)
{
    return inner_.tryLock(path);
}

#undef KATANA_FAULT_POINT

} // namespace katana::update
