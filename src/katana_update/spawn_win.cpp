// STUB. Task T7 writes the Windows Launcher of include/katana/update/process.hpp:
// CreateProcessW with the arguments quoted by its rules and never through a
// shell; run() waits with a timeout and merges the child's output;
// startDetached() leaves the child outside this process's job and console.
// CMake compiles this file on Windows only.
//
// CREATE_NO_WINDOW, in BOTH run() and startDetached(): katana_updater is a console program,
// and started from the window (a GUI program, no console of its own) Windows would open a
// black console for the length of the swap - and again for the preflight's --selftest. Task T9
// writes the helper and relies on this, but the file is T7's: T7 puts the flag in. The relaunch
// of katana.exe, a GUI program, needs no flag; DETACHED_PROCESS and CREATE_BREAKAWAY_FROM_JOB
// (when the job allows it) are what let the helper outlive the window.

#include "katana/update/process.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::unique_ptr<Launcher> makeSystemLauncher()
{
    return std::make_unique<detail::UnimplementedLauncher>("T7");
}

} // namespace katana::update
