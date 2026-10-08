#pragma once

// A program's command line as one string, by the rules Windows parses it with
// (CommandLineToArgvW, which every main() of the update system reads through
// utf8Arguments): INTERNAL to the update library and never installed.
//
// Two users, hence a header and not two copies. The Windows launcher (spawn_win.cpp) hands the
// string to CreateProcessW; the apply (apply.cpp) writes the same string to the RunOnce value
// that re-runs `katana_updater --recover <R>` at the next logon, and that string meets the
// same parser. Arguments are UTF-8; the quoting only treats the ASCII characters that matter
// (space, tab, quote, backslash), which no multi-byte UTF-8 sequence contains.
//
// The rule (Microsoft, "Parsing C++ command-line arguments"): an argument with no space, tab,
// line break or quote and not empty is written as it is; otherwise it is wrapped in quotes, a
// quote inside gets a backslash, and a run of backslashes that ends at a quote - an inner one
// or the closing one - is doubled.

#include <string>
#include <string_view>
#include <vector>

namespace katana::update::detail {

[[nodiscard]] inline std::string quoteArgument(std::string_view argument)
{
    if (!argument.empty() && argument.find_first_of(" \t\n\v\"") == std::string_view::npos) {
        return std::string(argument);
    }
    std::string out = "\"";
    std::size_t i = 0;
    while (true) {
        std::size_t backslashes = 0;
        while (i < argument.size() && argument[i] == '\\') {
            ++i;
            ++backslashes;
        }
        if (i == argument.size()) {
            out.append(backslashes * 2, '\\'); // before the closing quote
            break;
        }
        if (argument[i] == '"') {
            out.append(backslashes * 2 + 1, '\\'); // before an inner quote, and the quote itself
            out.push_back('"');
        } else {
            out.append(backslashes, '\\');
            out.push_back(argument[i]);
        }
        ++i;
    }
    out.push_back('"');
    return out;
}

// `"<program>" arg arg ...`. The program is always quoted: it is a path, and a path may hold a
// space. (CommandLineToArgvW reads the first token without backslash escapes, which is why it
// is wrapped in plain quotes and not run through quoteArgument; a Windows path holds no quote.)
[[nodiscard]] inline std::string commandLine(std::string_view program,
                                             const std::vector<std::string>& arguments)
{
    std::string line = "\"" + std::string(program) + "\"";
    for (const std::string& argument : arguments) {
        line += ' ';
        line += quoteArgument(argument);
    }
    return line;
}

} // namespace katana::update::detail
