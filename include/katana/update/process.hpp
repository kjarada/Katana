#pragma once

// Processes, behind two interfaces so that apply(), recover() and preflight()
// are driven in-process by tests with fakes.
//
//   ProcessOps  who is running (and which of them hold files under the
//               install root), and whether a pid is alive. The apply
//               algorithm waits on both and NEVER kills anything: it says who
//               holds the files and gives up (apply.hpp, step 2).
//               Windows: Toolhelp snapshot plus QueryFullProcessImageNameW;
//               Linux: /proc/<pid>/exe. Real implementations:
//               src/katana_updater/process_scan_win.cpp (T9) and
//               process_scan_posix.cpp (T18), compiled into the update
//               library (the program asks the same question to say `blocked`).
//   Launcher    starts a program: waits for it (the preflight self-test, the
//               recovery run) or starts it detached (the helper copy that
//               outlives the window, the relaunch of Katana). Real
//               implementations: src/katana_update/spawn_win.cpp and
//               spawn_posix.cpp.
//
// A program's file path is a std::filesystem::path; arguments are UTF-8 and
// reach the child as given (Windows: quoted by the CreateProcessW rules; never
// through a shell).

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::update {

struct RunningProgram {
    std::uint64_t pid = 0;
    std::filesystem::path image; // the executable or loaded module under the root
};

class ProcessOps {
  public:
    virtual ~ProcessOps() = default;

    // This process's id.
    [[nodiscard]] virtual std::uint64_t currentPid() const = 0;

    // True while `pid` names a live process.
    [[nodiscard]] virtual bool isAlive(std::uint64_t pid) const = 0;

    // Every process whose executable image is under `root` (case-insensitive
    // on Windows), excluding this process. Katana programs a client holds
    // open - katana_mcp - are among them.
    [[nodiscard]] virtual core::Result<std::vector<RunningProgram>>
    holdersUnder(const std::filesystem::path& root) const = 0;
};

struct SpawnRequest {
    std::filesystem::path executable;
    std::vector<std::string> arguments;
    std::filesystem::path workingDirectory; // empty: the child inherits ours
    // Waiting spawns only: kill the child and report timedOut after this long.
    std::chrono::milliseconds timeout{60'000};
};

struct SpawnResult {
    int exitCode = 0;
    bool timedOut = false;
    std::string output; // the child's standard output and error, merged, capped at 64 KiB
};

class Launcher {
  public:
    virtual ~Launcher() = default;

    // Runs the program to its end (or the timeout) and reports how it ended.
    // An error only when it could not be started.
    [[nodiscard]] virtual core::Result<SpawnResult> run(const SpawnRequest& request) = 0;

    // Starts the program and returns at once, the child detached from this
    // process's console, job and lifetime (it must outlive the window that
    // started it). `timeout` is ignored.
    [[nodiscard]] virtual core::Status startDetached(const SpawnRequest& request) = 0;
};

[[nodiscard]] std::unique_ptr<ProcessOps> makeSystemProcessOps();
[[nodiscard]] std::unique_ptr<Launcher> makeSystemLauncher();

// The engine's runningPrograms(root): makeSystemProcessOps()->holdersUnder(root).
[[nodiscard]] core::Result<std::vector<RunningProgram>>
runningPrograms(const std::filesystem::path& root);

} // namespace katana::update
