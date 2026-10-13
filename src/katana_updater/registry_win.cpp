// STUB. Task T9 writes the Windows registry of include/katana/update/registry.hpp
// (HKCU only: Software\Jarada\Katana, RunOnce, and the Uninstall key's
// DisplayVersion and DisplayName). Compiled into the update library, since the
// programs read installerLocation() to say `kind=installer`. CMake compiles
// this file on Windows only.

#include "katana/update/registry.hpp"

#include "../katana_update/not_implemented.hpp"

namespace katana::update {

std::unique_ptr<RegistryOps> makeSystemRegistryOps()
{
    return std::make_unique<detail::UnimplementedRegistryOps>("T9");
}

} // namespace katana::update
