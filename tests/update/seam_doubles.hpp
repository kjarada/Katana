#pragma once

// Two doubles for the seams of the swap that apply_harness.hpp does not have, shared by the
// tests of the swap and of its recovery (test_apply.cpp, test_recover.cpp). They belong beside
// FakeProcesses and HookFs in apply_harness.hpp, and are in a file of their own only because
// that one was not the hardening task's to edit (2026-10-14).
//
//   ScriptedProcesses  a process seam that answers each question from a list
//   BusyLockFs         a file system whose update lock is held by somebody else for a while

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <utility>
#include <vector>

#include "apply_harness.hpp"
#include "katana/core/error.hpp"
#include "katana/core/path_text.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/process.hpp"

namespace katana::update::harness {

// FakeProcesses can say "these programs, for the first n questions, and then nobody". The
// opposite - nobody at first, and somebody at a LATER question - is what a program started while
// the helper works looks like: Katana opened again while the plan is hashing, a script's
// katana_cli while a recovery is deciding. This seam gives each question its own answer, so a
// test names the one question at which the program is there.
class ScriptedProcesses final : public ProcessOps {
  public:
    // What one question about the programs under the root is answered with.
    struct Answer {
        std::vector<RunningProgram> holders; // empty: nobody is running
        bool fails = false;                  // the scan itself fails (no answer at all)
    };

    // The k-th question (from 1) gets the k-th answer; past the list nobody is running.
    explicit ScriptedProcesses(std::vector<Answer> answers = {}) : answers_(std::move(answers)) {}

    std::uint64_t pid = 1;
    // Called at each question with its number, before it is answered: where a test looks at the
    // disk as it stands at that moment.
    std::function<void(int)> onScan;
    mutable int scans = 0;

    [[nodiscard]] std::uint64_t currentPid() const override { return pid; }
    // No window is waited for in the tests that use this seam (waitPid is 0).
    [[nodiscard]] bool isAlive(std::uint64_t) const override { return false; }
    [[nodiscard]] core::Result<std::vector<RunningProgram>>
    holdersUnder(const std::filesystem::path&) const override
    {
        const int asked = ++scans;
        if (onScan) {
            onScan(asked);
        }
        if (asked > static_cast<int>(answers_.size())) {
            return std::vector<RunningProgram>{};
        }
        const Answer& answer = answers_[static_cast<std::size_t>(asked - 1)];
        if (answer.fails) {
            return core::makeError(core::ErrorCode::Internal, "the snapshot failed");
        }
        return answer.holders;
    }

  private:
    std::vector<Answer> answers_;
};

// One program of the install, for an answer.
inline ScriptedProcesses::Answer somebody(std::uint64_t pid, const char* image)
{
    return ScriptedProcesses::Answer{{RunningProgram{pid, core::pathFromUtf8(image)}}, false};
}

inline ScriptedProcesses::Answer nobody()
{
    return ScriptedProcesses::Answer{};
}

inline ScriptedProcesses::Answer noAnswer()
{
    return ScriptedProcesses::Answer{{}, true};
}

// Answers "held" to the first `heldFor` attempts to take the lock (always, when it is -1): what a
// helper that is alive looks like to another program, and what a virus scanner or an indexer
// that has the lock file open for a moment looks like to one that asks for it with share mode 0.
// `attempts` counts the questions, so a test can tell one attempt from a lock that was polled.
class BusyLockFs final : public ForwardingFs {
  public:
    BusyLockFs(FsOps& inner, int heldFor) : ForwardingFs(inner), heldFor_(heldFor) {}

    core::Result<std::unique_ptr<FileLock>> tryLock(const Path& p) override
    {
        ++attempts;
        if (heldFor_ < 0 || attempts <= heldFor_) {
            return core::makeError(core::ErrorCode::InvalidState, "held",
                                   "os_error=32 " + core::pathToUtf8(p));
        }
        return inner_.tryLock(p);
    }

    int attempts = 0;

  private:
    int heldFor_;
};

} // namespace katana::update::harness
