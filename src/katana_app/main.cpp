// katana_cli: the Katana engine without a GUI.
//
//   katana_cli                      interactive session on stdin
//   katana_cli script.kcs           run a script, one command per line
//   katana_cli -c "LINE 0,0 5,5" -c LIST
//
// Scripts and -c commands stop at the first failing command and exit with
// status 1, so the tool can be used in automated pipelines. Lines whose first
// non-blank is '#' are comments. It drives exactly the same Document, commands and storage
// as the desktop application; the verbs themselves are session.cpp's.
//
// Two things here are the update system's (update_verbs.hpp, session.hpp),
// and act on this program rather than on a drawing:
//   - before anything else, an update that was interrupted is put back
//     (recoverInterruptedUpdate);
//   - UPDATE APPLY CONFIRM must be the LAST line of a run. The line only
//     prepares: the run then ends, the session with it, and only then is the
//     update helper started, which waits for this process to exit before it
//     replaces a file. Nothing is started again afterwards. A script or a -c
//     with a line after it is refused before anything is prepared; typed or
//     piped on standard input, where the next line cannot be looked at
//     without waiting for it, the session ends with the line and says so.
//
// Text is UTF-8 here, and what reaches the program is made so at the door
// (docs/headless.md, "Text and names"):
//   - the arguments are read WIDE on Windows (update::utf8Arguments), where
//     argv holds the ANSI code page's version of them and a letter outside it
//     is already a '?': `-c "OPEN C:/Users/Łukasz/site"` is the path typed;
//   - a script is decoded as a text file is (Session::readScript);
//   - a console's lines are read wide as well (readLine), and its output is put
//     in UTF-8 for the run, so the names a reply prints are readable;
//   - any other line that is not UTF-8 - a pipe from a program writing the ANSI
//     code page - is read as Windows-1252 by Session::run, which says so.

#include <cstddef>
#include <iostream>
#include <iterator>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

#include "katana/core/text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/process.hpp"
#include "session.hpp"
#include "update_verbs.hpp"

namespace {

#if defined(_WIN32)

// The handle of standard input or output when it is a console, else null: a
// pipe or a file is read and written as the bytes it is.
HANDLE consoleHandle(DWORD which)
{
    const HANDLE handle = GetStdHandle(which);
    DWORD mode = 0;
    if (handle == nullptr || handle == INVALID_HANDLE_VALUE || !GetConsoleMode(handle, &mode)) {
        return nullptr;
    }
    return handle;
}

// A console shows the bytes a program writes through its OUTPUT code page,
// which is the OEM one (850, 437) unless set: a reply holding "Grüße" in UTF-8
// would be mojibake on the screen. Set to UTF-8 for the run and put back after,
// so the person's shell is as it was. Only for a console - a pipe is not
// transcoded by the page, and a program reading katana_cli's output reads the
// UTF-8 it always did.
class ConsoleOutputInUtf8 {
  public:
    ConsoleOutputInUtf8()
    {
        if (consoleHandle(STD_OUTPUT_HANDLE) != nullptr || consoleHandle(STD_ERROR_HANDLE) != nullptr) {
            before_ = GetConsoleOutputCP();
            if (before_ != 0 && before_ != CP_UTF8 && SetConsoleOutputCP(CP_UTF8)) {
                changed_ = true;
            }
        }
    }
    ~ConsoleOutputInUtf8()
    {
        if (changed_) {
            SetConsoleOutputCP(before_);
        }
    }
    ConsoleOutputInUtf8(const ConsoleOutputInUtf8&) = delete;
    ConsoleOutputInUtf8& operator=(const ConsoleOutputInUtf8&) = delete;

  private:
    UINT before_ = 0;
    bool changed_ = false;
};

#endif

// One line of standard input as UTF-8, without its terminator; false at the end
// of input. From a console the line is read as UTF-16 (ReadConsoleW) and
// converted: std::cin gets the bytes of the console's INPUT code page, so the
// "é" typed at the prompt arrived as 0x82 (OEM 850), which is no UTF-8 and no
// Windows-1252 either - "SAVE C:/Users/José/site" ended the session with the
// drawing unsaved.
bool readLine(std::string& line)
{
#if defined(_WIN32)
    static const HANDLE console = consoleHandle(STD_INPUT_HANDLE);
    if (console != nullptr) {
        std::wstring wide;
        for (;;) {
            wchar_t chunk[256];
            DWORD read = 0;
            if (!ReadConsoleW(console, chunk, static_cast<DWORD>(std::size(chunk)), &read,
                              nullptr) ||
                read == 0) {
                if (wide.empty()) {
                    return false; // the console was closed
                }
                break;
            }
            wide.append(chunk, read);
            if (wide.back() == L'\n') {
                break;
            }
        }
        // Ctrl+Z is the end of input, as it is to std::cin.
        if (wide.find(L'\x1A') != std::wstring::npos) {
            return false;
        }
        while (!wide.empty() && (wide.back() == L'\n' || wide.back() == L'\r')) {
            wide.pop_back();
        }
        line = katana::update::utf8FromWide(wide);
        return true;
    }
#endif
    return static_cast<bool>(std::getline(std::cin, line));
}

// Whether nothing is run after line `at`: every later line, up to a QUIT, is
// blank or a note.
bool isLastLineOfRun(const std::vector<std::string>& lines, std::size_t at)
{
    for (std::size_t later = at + 1; later < lines.size(); ++later) {
        if (katana::app::Session::isQuit(lines[later])) {
            break;
        }
        const std::string_view body = katana::core::trimmed(lines[later]);
        if (!body.empty() && body.front() != '#') {
            return false;
        }
    }
    return true;
}

int runBatch(katana::app::Session& session, const std::vector<std::string>& lines)
{
    for (std::size_t at = 0; at < lines.size(); ++at) {
        const std::string& line = lines[at];
        if (katana::app::Session::isQuit(line)) {
            break;
        }
        // Refused before it prepares anything: a line after it would be run
        // by a session that has already handed the install to the helper, or
        // not at all.
        if (katana::app::Session::isUpdateApply(line) && !isLastLineOfRun(lines, at)) {
            std::cerr << "error: Unsupported: UPDATE APPLY CONFIRM must be the last line of a "
                         "katana_cli run: the run ends with it and the update helper then "
                         "replaces the program, so put every other line before it\n";
            return 1;
        }
        if (!session.run(line)) {
            return 1;
        }
    }
    return 0;
}

} // namespace

int main(int argc, char* argv[])
{
    // First: a program must not read the files of an install that an
    // interrupted update left half swapped (session.hpp).
    katana::app::recoverInterruptedUpdate();

#if defined(_WIN32)
    const ConsoleOutputInUtf8 consoleOutput;
#endif
    // As UTF-8 and never from argv, which on Windows is the ANSI code page's
    // version of the command line (update/process.hpp): a path with an accented
    // letter in it ended the program in a filesystem_error, and one with a
    // letter outside the page - a Polish or Chinese profile name - was refused
    // as "not found" under a name of '?'s.
    const std::vector<std::string> arguments = katana::update::utf8Arguments(argc, argv);
    std::vector<std::string> batch;
    for (std::size_t i = 1; i < arguments.size(); ++i) {
        const std::string& argument = arguments[i];
        if (argument == "-h" || argument == "--help") {
            std::cout << "usage: katana_cli [script-file] [-c \"command\"]...\n\n"
                      << katana::app::Session::helpText();
            return 0;
        }
        if (argument == "-c") {
            if (i + 1 >= arguments.size()) {
                std::cerr << "error: -c needs a command\n";
                return 2;
            }
            batch.push_back(arguments[++i]);
            continue;
        }
        auto script = katana::app::Session::readScript(argument);
        if (!script) {
            std::cerr << "error: cannot read script " << argument << ": "
                      << script.error().describe() << '\n';
            return 2;
        }
        for (std::string& line : *script) {
            batch.push_back(std::move(line));
        }
    }

    // What an UPDATE APPLY CONFIRM left to be launched, taken out of the
    // session before it ends.
    std::optional<katana::update::ApplyRequest> apply;
    int status = 0;
    {
        // After the arguments, so that --help and a bad argument say nothing of
        // the customisation loaded. Never nullptr, which is a session of no
        // program and starts with no customisation (session.hpp): a katana_cli
        // started with no argv[0] is still katana_cli.
        katana::app::Session session(arguments.empty() ? "katana_cli"
                                                        : arguments.front().c_str());
        if (!batch.empty()) {
            status = runBatch(session, batch);
        } else {
            std::cout << "Katana command line. HELP lists commands, QUIT leaves.\n";
            for (std::string line; std::cout << "> " << std::flush, readLine(line);) {
                if (katana::app::Session::isQuit(line)) {
                    break;
                }
                (void)session.run(line); // interactive: report and carry on
                // Typed, an UPDATE APPLY CONFIRM that succeeded is the last
                // line by being it: the session ends here. That is said,
                // because a line after it is not run - one piped in behind it
                // is dropped here, where runBatch refuses the same lines
                // before anything is prepared. Standard input cannot be
                // looked ahead in: reading on would wait for a person who has
                // nothing more to type.
                if ((apply = session.takePendingApply())) {
                    std::cerr << "note: the session ends with UPDATE APPLY CONFIRM and the "
                                 "update helper is started now; a line typed or piped after it "
                                 "is not run\n";
                    break;
                }
            }
        }
        if (status == 0 && !apply.has_value()) {
            apply = session.takePendingApply();
        }
    }

    // The session has ended and its drawing is closed. Only now is the helper
    // started; it waits for this process to exit, and this process starts
    // nothing after it.
    if (apply.has_value()) {
        if (const katana::core::Status launched = katana::update::launchApply(*apply); !launched) {
            // The helper's copy the line had made is nobody's now: it is listed
            // for the sweep (update_verbs.hpp, abandonApply).
            (void)katana::app::abandonApply(*apply);
            std::cerr << "error: the update helper could not be started, so nothing was "
                         "installed and the release stays downloaded: "
                      << launched.error().describe() << '\n';
            return 1;
        }
    }
    return status;
}
