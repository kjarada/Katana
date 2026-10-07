// STUB. Task T7 writes the real file system of include/katana/update/fsops.hpp
// for Windows: MoveFileExW without a delayed-until-reboot flag and with the
// retry on errors 5, 32 and 33, FlushFileBuffers, a lock opened with share
// mode 0, reparse-point detection. CMake compiles this file on Windows only.

#include "katana/update/fsops.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::unique_ptr<FsOps> makeSystemFsOps(const RetryPolicy&)
{
    return std::make_unique<detail::UnimplementedFsOps>("T7");
}

} // namespace katana::update
