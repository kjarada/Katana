// STUB. Task T6 writes the install records (install.hpp) and the per-user state
// (state.hpp): readInstall, installRootOfRunningProgram, the path helpers,
// state.json and last.json. They share this file because they are all small
// JSON records read and written the same way.

#include "katana/update/install.hpp"
#include "katana/update/state.hpp"

#include "not_implemented.hpp"

namespace katana::update {

// ---- install.hpp --------------------------------------------------------------

std::string_view toString(InstallKind)
{
    detail::notYet("toString(InstallKind)", "T6");
}

std::filesystem::path installRecordDirectory(const std::filesystem::path&)
{
    detail::notYet("installRecordDirectory", "T6");
}

std::filesystem::path updateWorkDirectory(const std::filesystem::path&)
{
    detail::notYet("updateWorkDirectory", "T6");
}

core::Result<Install> readInstall(const std::filesystem::path&, const RegistryOps&)
{
    return detail::notImplemented("readInstall", "T6");
}

core::Result<std::filesystem::path> installRootOfRunningProgram()
{
    return detail::notImplemented("installRootOfRunningProgram", "T6");
}

// ---- state.hpp ----------------------------------------------------------------

std::filesystem::path stateDirectory()
{
    detail::notYet("stateDirectory", "T6");
}

std::filesystem::path stagePath(const std::filesystem::path&, std::uint64_t)
{
    detail::notYet("stagePath", "T6");
}

std::filesystem::path runPath(const std::filesystem::path&, const std::string&)
{
    detail::notYet("runPath", "T6");
}

std::filesystem::path healthMarkerPath(const std::filesystem::path&, const std::string&)
{
    detail::notYet("healthMarkerPath", "T6");
}

core::Result<UpdateState> readState(const std::filesystem::path&)
{
    return detail::notImplemented("readState", "T6");
}

core::Status writeState(const std::filesystem::path&, const UpdateState&)
{
    return detail::notImplemented("writeState", "T6");
}

core::Result<std::optional<LastResult>> readLastResult(const std::filesystem::path&)
{
    return detail::notImplemented("readLastResult", "T6");
}

core::Status writeLastResult(const std::filesystem::path&, const LastResult&)
{
    return detail::notImplemented("writeLastResult", "T6");
}

core::Status clearLastResult(const std::filesystem::path&)
{
    return detail::notImplemented("clearLastResult", "T6");
}

} // namespace katana::update
