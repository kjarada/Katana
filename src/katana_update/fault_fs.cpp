// STUB. Task T7 writes the fault-injecting FsOps of
// include/katana/update/fault_fs.hpp. Every operation is refused until then:
// a wrapper that passed straight through while ignoring crashBefore() would
// make a fault loop that injects nothing and passes.

#include "katana/update/fault_fs.hpp"

#include "not_implemented.hpp"

namespace katana::update {

FaultFs::FaultFs(FsOps& inner) : inner_(inner) {}

void FaultFs::crashBefore(std::uint64_t)
{
    detail::notYet("FaultFs::crashBefore", "T7");
}

void FaultFs::failRenameWith(std::uint64_t, int)
{
    detail::notYet("FaultFs::failRenameWith", "T7");
}

void FaultFs::reset()
{
    detail::notYet("FaultFs::reset", "T7");
}

std::uint64_t FaultFs::mutatingOperations() const
{
    detail::notYet("FaultFs::mutatingOperations", "T7");
}

core::Result<EntryInfo> FaultFs::stat(const std::filesystem::path&) const
{
    return detail::notImplemented("FaultFs::stat", "T7");
}

core::Result<std::vector<std::filesystem::path>>
FaultFs::listDirectory(const std::filesystem::path&) const
{
    return detail::notImplemented("FaultFs::listDirectory", "T7");
}

core::Result<std::string> FaultFs::readFile(const std::filesystem::path&) const
{
    return detail::notImplemented("FaultFs::readFile", "T7");
}

core::Result<std::filesystem::path> FaultFs::resolve(const std::filesystem::path&) const
{
    return detail::notImplemented("FaultFs::resolve", "T7");
}

core::Result<std::uint64_t> FaultFs::freeSpace(const std::filesystem::path&) const
{
    return detail::notImplemented("FaultFs::freeSpace", "T7");
}

core::Status FaultFs::makeDirectories(const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::makeDirectories", "T7");
}

core::Status FaultFs::copyFile(const std::filesystem::path&, const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::copyFile", "T7");
}

core::Status FaultFs::renameFile(const std::filesystem::path&, const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::renameFile", "T7");
}

core::Status FaultFs::removeFile(const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::removeFile", "T7");
}

core::Status FaultFs::removeEmptyDirectory(const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::removeEmptyDirectory", "T7");
}

core::Status FaultFs::appendDurable(const std::filesystem::path&, std::string_view)
{
    return detail::notImplemented("FaultFs::appendDurable", "T7");
}

core::Status FaultFs::writeDurable(const std::filesystem::path&, std::string_view)
{
    return detail::notImplemented("FaultFs::writeDurable", "T7");
}

core::Status FaultFs::setExecutable(const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::setExecutable", "T7");
}

core::Status FaultFs::syncDirectory(const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::syncDirectory", "T7");
}

core::Result<std::unique_ptr<FileLock>> FaultFs::tryLock(const std::filesystem::path&)
{
    return detail::notImplemented("FaultFs::tryLock", "T7");
}

} // namespace katana::update
