// The Launcher of include/katana/update/process.hpp on Linux and macOS, which is NOT WRITTEN
// YET. Task T18 writes it (posix_spawn or fork and exec, setsid for a detached child). CMake
// compiles this file everywhere except Windows.
//
// Until it is written both operations REFUSE, with the code Unsupported, and start nothing. An
// error is the only honest answer here: run() that returned an exit code would be telling a
// preflight that a helper passed its self-test, or a swap that the new version started, and
// startDetached() that returned success would let a window close for a helper that never ran.
// (The file system of these platforms refuses first in every real path - fsops_posix.cpp - so
// this is reached only by a caller that holds a launcher and no file system: launchApply.)

#include "katana/update/process.hpp"

#include <memory>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

namespace {

core::Error refusal(std::string_view operation)
{
    return core::makeError(core::ErrorCode::Unsupported,
                           "Katana does not start its update helper on this system: updates are "
                           "checked for here, and installed by hand",
                           "os_error=0 Launcher::" + std::string(operation));
}

class NoLauncher final : public Launcher {
  public:
    [[nodiscard]] core::Result<SpawnResult> run(const SpawnRequest&) override
    {
        return refusal("run");
    }
    [[nodiscard]] core::Status startDetached(const SpawnRequest&) override
    {
        return refusal("startDetached");
    }
};

} // namespace

std::unique_ptr<Launcher> makeSystemLauncher()
{
    return std::make_unique<NoLauncher>();
}

} // namespace katana::update
