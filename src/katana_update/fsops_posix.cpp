// STUB. Task T18 writes the real file system of include/katana/update/fsops.hpp
// for Linux (and macOS, where it is only compiled, never used to apply):
// rename(2) plus fsync of the directory, flock, symlink detection. CMake
// compiles this file everywhere except Windows.

#include "katana/update/fsops.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::unique_ptr<FsOps> makeSystemFsOps(const RetryPolicy&)
{
    return std::make_unique<detail::UnimplementedFsOps>("T18");
}

} // namespace katana::update
