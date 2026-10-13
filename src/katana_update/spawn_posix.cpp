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
//
// The refusal's context begins `reason=platform`, the token stage() and preflight() refuse under
// here (stage.cpp), so that launchApply's caller reads the same reason with fetchFailureReason
// (feed.hpp) as theirs. Without the token that function reads an Unsupported as `too_large`, a
// download that was too big, which is nothing a launcher can mean.

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
                           "reason=platform; os_error=0 Launcher::" + std::string(operation));
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

// The elevated half of a split token is a thing of Windows (process.hpp): there is none here,
// and no update is applied here either (`reason=platform`, above).
bool processIsElevated()
{
    return false;
}

} // namespace katana::update
