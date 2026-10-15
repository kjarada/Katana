// The one geoprocessing executor (geo_verbs.hpp, docs/geoprocessing.md "The
// executor").

#include "geo_verbs.hpp"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <cstdint>
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

// The row that takes this line: its verb's, unless that row leaves the line
// to the interpreter (INFO <id>).
const VerbEntry* entryTaking(const Tokens& tokens)
{
    const VerbEntry* entry = tokens.words.empty() ? nullptr : entryFor(tokens.words);
    return entry != nullptr && (entry->takes == nullptr || entry->takes(tokens)) ? entry : nullptr;
}

// ---- the drawing's surfaces, staged (geo_verbs.hpp) ----------------------------------

// The undo step a line's change of the surfaces is named: its verb, and its
// second word when that is a word ("SURFACE FROM", "GDAL RASTER") and not a
// path or a name.
std::string stepNameOf(const Tokens& tokens)
{
    std::string name = upper(tokens.words.front());
    if (tokens.size() > 1 && !tokens.quoted[1] && !tokens[1].empty() &&
        std::ranges::all_of(tokens[1], [](char c) {
            return std::isalpha(static_cast<unsigned char>(c)) != 0;
        })) {
        name += " " + upper(tokens[1]);
    }
    return name;
}

// A copy of the drawing's surfaces for the phase about to run; its revision,
// to tell afterwards whether the phase changed it. Dropped when the phase
// ends, on every path out of it (geo_verbs.hpp: "nothing keeps it between
// phases"): the copy holds a shared_ptr to every surface, and the Context
// lives as long as the window or the session, so a copy left behind kept the
// previous drawing's surfaces in memory after NEW until the next geo line.
class Staging {
  public:
    explicit Staging(Context& context) : context_(context)
    {
        context_.surfaces = context_.document.surfaces();
        revision_ = context_.surfaces.revision();
    }
    ~Staging() { context_.surfaces = katana::terrain::SurfaceStore{}; }
    Staging(const Staging&) = delete;
    Staging& operator=(const Staging&) = delete;

    [[nodiscard]] std::uint64_t revision() const { return revision_; }

  private:
    Context& context_;
    std::uint64_t revision_ = 0;
};

// What the phase changed, committed as one step. Nothing when it changed
// nothing, or when the drawing's surfaces already are the staged ones (an
// apply that committed them in a step of its own): Document::setSurfaces
// records no step for an equal store.
katana::core::Status commit(Context& context, std::uint64_t staged, const std::string& step)
{
    if (context.surfaces.revision() == staged) {
        return {};
    }
    return context.document.setSurfaces(context.surfaces, step);
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
    return !tokens->words.empty() && !tokens->quoted[0] && entryTaking(*tokens) != nullptr;
}

Result<Prepared> prepare(Context& context, std::string_view line)
{
    auto tokens = tokenize(line);
    if (!tokens) {
        return tokens.error();
    }
    const VerbEntry* entry = entryTaking(*tokens);
    if (entry == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "not a geoprocessing verb",
                         std::string(katana::core::trimmed(line)));
    }
    const std::string step = stepNameOf(*tokens);
    const Staging staging(context);
    auto prepared = entry->prepare(context, *tokens, line);
    if (!prepared) {
        return prepared;
    }
    // A line answered here may have changed the surfaces (SURFACE REMOVE).
    if (auto status = commit(context, staging.revision(), step); !status) {
        return status.error();
    }
    if (prepared->work) {
        // The apply, wherever it runs - inline here, or on the window's GUI
        // thread when its job ends - stages the drawing's surfaces as they
        // are THEN, and commits what it changed. The work itself is left as
        // it was: it never sees the Context.
        prepared->work = [work = std::move(prepared->work),
                          step](const std::stop_token& stop,
                                const Progress& progress) -> Result<Apply> {
            auto apply = work(stop, progress);
            if (!apply) {
                return apply.error();
            }
            return Apply([apply = std::move(apply).value(),
                          step](Context& ctx) -> Result<std::string> {
                const Staging applyStaging(ctx);
                auto reply = apply(ctx);
                if (!reply) {
                    return reply;
                }
                if (auto status = commit(ctx, applyStaging.revision(), step); !status) {
                    return status.error();
                }
                return reply;
            });
        };
    }
    return prepared;
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

std::filesystem::path ownScratch()
{
    // The process's folder is named by the process id AND the time it was
    // first asked for: Windows hands an ended process's id to a later one,
    // which found the folder the earlier one left behind with its files
    // still in it, and a result never replaces a file (keepDerived), so its
    // `west` came back as `west-2`. Two processes with one id never run at
    // once, so the later one's clock reads later. Inside it each front end
    // has a folder of its own, for the same reason: two Sessions in one
    // process (katana_geo_tests runs dozens) shared one folder, and the
    // second's `west` came back as `west-2`.
    static const std::filesystem::path process = [] {
        std::error_code error;
        std::filesystem::path temp = std::filesystem::temp_directory_path(error);
        if (error) {
            temp = std::filesystem::current_path(error);
        }
#if defined(_WIN32)
        const auto id = static_cast<long long>(_getpid());
#else
        const auto id = static_cast<long long>(getpid());
#endif
        const auto started = std::chrono::duration_cast<std::chrono::nanoseconds>(
                                 std::chrono::system_clock::now().time_since_epoch())
                                 .count();
        return temp / "katana-scratch" / (std::to_string(id) + "-" + std::to_string(started));
    }();
    static std::atomic<unsigned long long> served{0};
    return process / std::to_string(++served);
}

} // namespace katana::app::geo
