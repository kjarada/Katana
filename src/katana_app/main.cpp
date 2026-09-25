// katana_cli: the Katana engine without a GUI.
//
//   katana_cli                      interactive session on stdin
//   katana_cli script.kcs           run a script, one command per line
//   katana_cli -c "LINE 0,0 5,5" -c LIST
//
// Scripts and -c commands stop at the first failing command and exit with
// status 1, so the tool can be used in automated pipelines. Lines starting with
// '#' are comments. It drives exactly the same Document, commands and storage
// as the desktop application; the verbs themselves are session.cpp's.

#include <fstream>
#include <iostream>
#include <string>
#include <utility>
#include <vector>

#include "session.hpp"

namespace {

int runBatch(katana::app::Session& session, const std::vector<std::string>& lines)
{
    for (const std::string& line : lines) {
        if (katana::app::Session::isQuit(line)) {
            break;
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

    // After the arguments, so that --help and a bad argument say nothing of
    // the customisation loaded.
    katana::app::Session session(argc > 0 ? argv[0] : nullptr);
    if (!batch.empty()) {
        return runBatch(session, batch);
    }

    std::cout << "Katana command line. HELP lists commands, QUIT leaves.\n";
    for (std::string line; std::cout << "> " << std::flush, std::getline(std::cin, line);) {
        if (katana::app::Session::isQuit(line)) {
            break;
        }
        (void)session.run(line); // interactive: report and carry on
    }
    return 0;
}
