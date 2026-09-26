// The one geoprocessing executor (geo_verbs.hpp, docs/geoprocessing.md "The
// executor").

#include "geo_verbs.hpp"

#include <cctype>
#include <chrono>
#include <filesystem>
#include <string>
#include <system_error>
#include <utility>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/core/text.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
}

// The row that takes a line of these words: one whose second word matches
// before one that takes any.
const VerbEntry* entryFor(const std::vector<std::string>& words)
{
    if (words.empty()) {
        return nullptr;
    }
    const std::string verb = upper(words[0]);
    const std::string second = words.size() > 1 ? upper(words[1]) : std::string();
    const VerbEntry* any = nullptr;
    for (const VerbEntry& entry : verbTable()) {
        if (verb != entry.verb) {
            continue;
        }
        if (std::string_view(entry.subverb).empty()) {
            any = any != nullptr ? any : &entry;
        } else if (second == entry.subverb) {
            return &entry;
        }
    }
    return any;
}

} // namespace

bool handles(std::string_view line)
{
    auto tokens = tokenize(line);
    if (!tokens) {
        // A quote left open still says which verb it is; the verb refuses it.
        const std::string_view body = katana::core::trimmed(line);
        const std::size_t blank = body.find_first_of(" \t");
        return entryFor({std::string(body.substr(0, blank))}) != nullptr;
    }
    return !tokens->words.empty() && !tokens->quoted[0] && entryFor(tokens->words) != nullptr;
}

Result<Prepared> prepare(Context& context, std::string_view line)
{
    auto tokens = tokenize(line);
    if (!tokens) {
        return tokens.error();
    }
    const VerbEntry* entry = tokens->words.empty() ? nullptr : entryFor(tokens->words);
    if (entry == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "not a geoprocessing verb",
                         std::string(katana::core::trimmed(line)));
    }
    return entry->prepare(context, *tokens, line);
}

Result<std::string> runNow(Context& context, std::string_view line, const std::stop_token& stop,
                           const Progress& progress)
{
    auto prepared = prepare(context, line);
    if (!prepared) {
        return prepared.error();
    }
    if (prepared->reply) {
        return *prepared->reply;
    }
    auto apply = prepared->work(stop, progress);
    if (!apply) {
        return apply.error();
    }
    return (*apply)(context);
}

std::string helpText()
{
    std::string text;
    for (const VerbEntry& entry : verbTable()) {
        text += (text.empty() ? "Geo       " : "          ") + std::string(entry.usage) + "\n";
    }
    return text;
}

std::filesystem::path defaultScratch()
{
    // Worked out once, so every front end in one process keeps its rasters in
    // one folder. Named by the process id AND the time it was first asked
    // for: Windows hands an ended process's id to a later one, which found the
    // folder the earlier one left behind with its files still in it, and a
    // result never replaces a file (keepDerived), so its `west` came back as
    // `west-2`. Two processes with one id never run at once, so the later
    // one's clock reads later.
    static const std::filesystem::path folder = [] {
        std::error_code error;
        std::filesystem::path temp = std::filesystem::temp_directory_path(error);
        if (error) {
            temp = std::filesystem::current_path(error);
        }
#if defined(_WIN32)
        const auto process = static_cast<long long>(_getpid());
#else
        const auto process = static_cast<long long>(getpid());
#endif
        const auto started = std::chrono::duration_cast<std::chrono::nanoseconds>(
                                 std::chrono::system_clock::now().time_since_epoch())
                                 .count();
        return temp / "katana-scratch" /
               (std::to_string(process) + "-" + std::to_string(started));
    }();
    return folder;
}

} // namespace katana::app::geo
