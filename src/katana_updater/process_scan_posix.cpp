// STUB. Task T18 writes the POSIX process scan of include/katana/update/process.hpp:
// /proc/<pid>/exe and the maps of each process, kill(pid, 0) for liveness.
// Compiled into the update library, like process_scan_win.cpp; CMake compiles
// this file everywhere except Windows.

#include "katana/update/process.hpp"

#include "../katana_update/not_implemented.hpp"

namespace katana::update {

std::unique_ptr<ProcessOps> makeSystemProcessOps()
{
    return std::make_unique<detail::UnimplementedProcessOps>("T18");
}

core::Result<std::vector<RunningProgram>> runningPrograms(const std::filesystem::path& root)
{
    return makeSystemProcessOps()->holdersUnder(root);
}

} // namespace katana::update
