// The process seam of include/katana/update/process.hpp on Linux and macOS. Two of its three
// questions are answered here; the third is NOT WRITTEN YET (task T18: /proc/<pid>/exe and the
// maps of each process on Linux). Compiled into the update library, like process_scan_win.cpp;
// CMake compiles this file everywhere except Windows.
//
//   currentPid    getpid(). The interface returns a number and has no way to say "not here", so
//                 the only other thing this could do is throw - at whichever program asked for
//                 its own id, on a platform where it has every right to ask.
//   isAlive       kill(pid, 0), which sends nothing and reports whether the process is there.
//                 The interface returns a bool: "false" of a live process is the one answer that
//                 would let a swap start under a running program, so it is the true one or none.
//                 A process that has ended and has not been reaped by its parent yet (a zombie)
//                 still counts as alive; the swap that waits on this is T18's, and so is the
//                 choice of what to do about it.
//   holdersUnder  REFUSES, with the code Unsupported. Which programs run from an install is the
//                 question the scan answers, and "none" from a function that did not look would
//                 be read as "nothing is running, go on". apply() takes the refusal for what it
//                 is: `scan_failed`, nothing touched. Its context begins `reason=platform`, so
//                 that a caller of runningPrograms() reads the reason stage() and preflight()
//                 give here (fetchFailureReason, feed.hpp) and not `too_large`, which is what
//                 that function makes of an Unsupported with no token.

#include "katana/update/process.hpp"

#include <cerrno>
#include <cstdint>
#include <filesystem>
#include <limits>
#include <memory>
#include <vector>

#include <signal.h>
#include <sys/types.h>
#include <unistd.h>

#include "katana/core/error.hpp"

namespace katana::update {

namespace {

class PosixProcessOps final : public ProcessOps {
  public:
    [[nodiscard]] std::uint64_t currentPid() const override
    {
        return static_cast<std::uint64_t>(::getpid());
    }

    [[nodiscard]] bool isAlive(std::uint64_t pid) const override
    {
        // 0 is "every process of this group" to kill(), and a number too wide for a pid_t would
        // become another process's id, or a negative one, which kill() reads as a group:
        // neither is put to it. (process.hpp says the same of the Windows scan.)
        if (pid == 0 || pid > static_cast<std::uint64_t>(std::numeric_limits<pid_t>::max())) {
            return false;
        }
        if (::kill(static_cast<pid_t>(pid), 0) == 0) {
            return true;
        }
        // EPERM: the process is there and is another user's. Otherwise (ESRCH) there is none.
        return errno == EPERM;
    }

    [[nodiscard]] core::Result<std::vector<RunningProgram>>
    holdersUnder(const std::filesystem::path&) const override
    {
        return core::makeError(core::ErrorCode::Unsupported,
                               "the programs running from an install cannot be listed on this "
                               "system yet",
                               "reason=platform; os_error=0 ProcessOps::holdersUnder");
    }
};

} // namespace

std::unique_ptr<ProcessOps> makeSystemProcessOps()
{
    return std::make_unique<PosixProcessOps>();
}

core::Result<std::vector<RunningProgram>> runningPrograms(const std::filesystem::path& root)
{
    return makeSystemProcessOps()->holdersUnder(root);
}

} // namespace katana::update
