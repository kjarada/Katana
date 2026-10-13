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
//               spawn_posix.cpp. On Windows the child is started with
//               CREATE_NO_WINDOW, for the helper is a console program and its
//               parent is a GUI program: without the flag a black console opens
//               for the length of every swap and every self-test.
//
// A program's file path is a std::filesystem::path; arguments are UTF-8 and
// reach the child as given (Windows: quoted by the CreateProcessW rules; never
// through a shell).
//
// WHAT THE WINDOWS SCAN ANSWERS (src/katana_updater/process_scan_win.cpp, which
// gives the reasons):
//   holdersUnder  the processes STARTED FROM a file under the root, and for each
//                 RunningProgram::image is that executable - never a module it
//                 loaded: what must not live through a swap is a program of the
//                 old version, the commit only renames (which a loaded DLL
//                 allows), and listing the modules of every process fails on
//                 exactly the protected ones. The root may be spelled in any way
//                 that names the folder (either case, forward slashes, a
//                 trailing separator, "." and ".." segments, the extended-length
//                 prefix, 8.3 names, a junction); the paths are put in one form
//                 and compared with the file system's own case folding, and
//                 C:/Katana2 is not under C:/Katana. A root that is not there
//                 holds nothing; an empty one is InvalidArgument. A process that
//                 has ended is not listed. Nor is one this user may not open
//                 (another account's): it gives no image path and is passed over,
//                 NOT taken for a holder - and because the file of a running
//                 program can be renamed, the update goes through under it
//                 (process_scan_win.cpp says what that leaves, and why a file's
//                 name alone is not held on). A snapshot that cannot be taken or
//                 read is an error, never "no holders": apply() then refuses with
//                 "scan_failed". No elevation is ever asked for.
//   isAlive       asks about the ID, which Windows may hand to a new process
//                 once the old one is gone; the interface has no creation time to
//                 tell them apart, and the worst case is a wait for the wrong
//                 process that ends as "blocked". A process that exists but may
//                 not be opened is alive; 0, and a number wider than a process
//                 id, are not.

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <string_view>
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

// The program's own command line as UTF-8, argv[0] first. EVERY main() of the update
// system (katana_updater, katana_update_e2e, katana_updater_test) reads its arguments
// through this and not through argv: on Windows argv holds the ANSI code page's version of
// the command line, so a profile folder with an accented letter in its name reaches the
// helper as '?' or mojibake, and the helper is exactly the program started with such paths
// (--root, --stage, --state). Windows: CommandLineToArgvW(GetCommandLineW()) converted to
// UTF-8 (argc and argv are ignored); elsewhere argv as given, which is UTF-8 or raw bytes
// by the file system's own rule. A path is made of one with core::pathFromUtf8
// (core/path_text.hpp), never by handing the string to std::filesystem::path, which on
// Windows throws at the first byte that is not UTF-8. Finished, with tests:
// src/katana_update/arguments.cpp.
[[nodiscard]] std::vector<std::string> utf8Arguments(int argc, char** argv);

#ifdef _WIN32
// UTF-16 to UTF-8 (the conversion above, exposed so that it can be tested with text no
// command line is needed to carry). A well-formed surrogate pair becomes one 4-byte
// sequence; an unpaired surrogate becomes U+FFFD (EF BF BD).
[[nodiscard]] std::string utf8FromWide(std::wstring_view wide);
#endif

[[nodiscard]] std::unique_ptr<ProcessOps> makeSystemProcessOps();
[[nodiscard]] std::unique_ptr<Launcher> makeSystemLauncher();

// The engine's runningPrograms(root): makeSystemProcessOps()->holdersUnder(root).
[[nodiscard]] core::Result<std::vector<RunningProgram>>
runningPrograms(const std::filesystem::path& root);

// Whether this process holds the ELEVATED HALF OF A SPLIT TOKEN: a program started "as
// administrator" on a Windows machine where an administrator's programs ordinarily run with
// the other, filtered half. The helper runs from a copy in the per-user update folder
// (stage.hpp, preflight), which those ordinary programs can write: started from such a
// process it would run with administrator rights from a folder that programs without them
// control. So `UPDATE APPLY CONFIRM` is refused there, before the copy is made
// (src/katana_app/update_verbs.hpp, reason=elevated).
//
// Windows: GetTokenInformation(TokenElevationType) answers TokenElevationTypeFull. A machine
// with that protection turned off, and the built-in administrator account, have ONE token and
// no unelevated half that could interfere (TokenElevationTypeDefault): false, and an update
// is applied there as anywhere. A process that could not ask - its token cannot be opened or
// read - is answered false as well, and that is no error: the question guards one case, and
// what cannot be shown to be that case is not refused an update for it. Elsewhere than
// Windows: false. Asked once by makeUpdateContext; defined in spawn_win.cpp and
// spawn_posix.cpp.
[[nodiscard]] bool processIsElevated();

} // namespace katana::update
