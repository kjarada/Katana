// STUB. Task T9 writes the Windows process scan of include/katana/update/process.hpp:
// a Toolhelp snapshot, QueryFullProcessImageNameW for each process and for the
// modules of those it may open, OpenProcess and WaitForSingleObject for
// liveness.
//
// This file lives with the helper, but the UPDATE LIBRARY compiles it
// (src/katana_update/CMakeLists.txt), because katana, katana_cli and
// katana_mcp ask the same question to say `blocked`, and runningPrograms() is
// part of the engine's API. CMake compiles it on Windows only.

#include "katana/update/process.hpp"

#include "../katana_update/not_implemented.hpp"

namespace katana::update {

std::unique_ptr<ProcessOps> makeSystemProcessOps()
{
    return std::make_unique<detail::UnimplementedProcessOps>("T9");
}

core::Result<std::vector<RunningProgram>> runningPrograms(const std::filesystem::path& root)
{
    return makeSystemProcessOps()->holdersUnder(root);
}

} // namespace katana::update
