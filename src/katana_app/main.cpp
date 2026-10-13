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

#include <cstddef>
#include <fstream>
#include <iostream>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/update/apply.hpp"
#include "session.hpp"
#include "update_verbs.hpp"

namespace {

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

    std::vector<std::string> batch;
    for (int i = 1; i < argc; ++i) {
        const std::string argument = argv[i];
        if (argument == "-h" || argument == "--help") {
            std::cout << "usage: katana_cli [script-file] [-c \"command\"]...\n\n"
                      << katana::app::Session::helpText();
            return 0;
        }
        if (argument == "-c") {
            if (i + 1 >= argc) {
                std::cerr << "error: -c needs a command\n";
                return 2;
            }
            batch.emplace_back(argv[++i]);
            continue;
        }
        std::ifstream script(argument);
        if (!script) {
            std::cerr << "error: cannot read script " << argument << '\n';
            return 2;
        }
        for (std::string line; std::getline(script, line);) {
            if (!line.empty() && line.back() == '\r') {
                line.pop_back(); // scripts saved with Windows line endings
            }
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
        katana::app::Session session(argc > 0 ? argv[0] : "katana_cli");
        if (!batch.empty()) {
            status = runBatch(session, batch);
        } else {
            std::cout << "Katana command line. HELP lists commands, QUIT leaves.\n";
            for (std::string line;
                 std::cout << "> " << std::flush, std::getline(std::cin, line);) {
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
