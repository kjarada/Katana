// STUB. Task T18 writes the POSIX Launcher of include/katana/update/process.hpp
// (posix_spawn or fork and exec, setsid for a detached child). CMake compiles
// this file everywhere except Windows.

#include "katana/update/process.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::unique_ptr<Launcher> makeSystemLauncher()
{
    return std::make_unique<detail::UnimplementedLauncher>("T18");
}

} // namespace katana::update
