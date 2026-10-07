// STUB. Task T7 writes the Windows Launcher of include/katana/update/process.hpp:
// CreateProcessW with the arguments quoted by its rules and never through a
// shell; run() waits with a timeout and merges the child's output;
// startDetached() leaves the child outside this process's job and console.
// CMake compiles this file on Windows only.

#include "katana/update/process.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::unique_ptr<Launcher> makeSystemLauncher()
{
    return std::make_unique<detail::UnimplementedLauncher>("T7");
}

} // namespace katana::update
