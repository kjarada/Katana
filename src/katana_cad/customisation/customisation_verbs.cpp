// CUSTOMISE (customisation_verbs.hpp): one family for every front end.

#include "katana/cad/customisation_verbs.hpp"

#include <algorithm>
#include <cstddef>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <set>
#include <system_error>
#include <utility>

#include "katana/cad/customisation_merge.hpp"
#include "katana/cad/customisation_part.hpp"
#include "katana/cad/customisation_record.hpp"
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/definition_users.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/linework_codes.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Customisation;
namespace fs = std::filesystem;
using Words = std::vector<std::string>;

namespace {

constexpr const char* kLoadUsage = "CUSTOMISE [REPLACE] <file> [<file>...]";
constexpr const char* kExportUsage = "CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] "
                                     "[NAME <name>] [ONLY <definition>...]";
constexpr const char* kRemoveUsage =
    "CUSTOMISE REMOVE <definition>... [FORCE] | CUSTOMISE REMOVE CODE <key>...";
constexpr const char* kSetUsage =
    "CUSTOMISE SET <key>=<value>...; keys: auto.codes=on|off auto.linework=on|off "
    "linework.start|end|close|arcstart|arcend|join|rectangle=<word>";

// How many names of a list a reply prints before it only counts the rest: a
// Replace of the whole library removes hundreds.
constexpr std::size_t kNamesListed = 20;

katana::core::Error usage(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

bool is(const std::string& word, std::string_view name)
{
    return katana::core::equalsIgnoringCase(word, name);
}

std::string number(std::size_t value) { return std::to_string(value); }
const char* yesNo(bool value) { return value ? "yes" : "no"; }

// A path in a record: with '/', on every platform. A backslash in a record
// is an escape (recordValue), so C:\site\codes.json would be written with
// every one doubled and read by nobody.
std::string pathValue(const fs::path& path)
{
    const std::u8string text = path.generic_u8string();
    return recordValue(std::string(text.begin(), text.end()));
}

// " definitions=<n> codes=<n> rules=<n>": what the session holds now.
std::string countsOf(const Document& document)
{
    return " definitions=" + number(document.styleLibrary().size()) +
           " codes=" + number(document.surveyMap().keys().size()) +
           " rules=" + number(document.surveyMap().size());
}

// `names` once each, in the order first given.
Words distinct(const Words& names)
{
    Words once;
    for (const std::string& name : names) {
        if (std::find(once.begin(), once.end(), name) == once.end()) {
            once.push_back(name);
        }
    }
    return once;
}

// "<what> <count key>=<n>" and then up to kNamesListed records naming one
// each, so that a reply says how many and shows enough of them to act on.
void listNames(std::string& reply, const char* what, const char* key, const Words& names)
{
    for (std::size_t i = 0; i < names.size() && i < kNamesListed; ++i) {
        reply += std::string(what) + " " + key + "=" + recordValue(names[i]) + "\n";
    }
}

// ---- files -------------------------------------------------------------------------------

fs::path beside(const fs::path& file, const char* suffix)
{
    fs::path other = file;
    other += suffix;
    return other;
}

// The digest of the bytes of `file`; nothing when it is not there, or cannot
// be read (a KEEP then finds it there and refuses for that).
std::optional<std::string> digestOfFile(const fs::path& file)
{
    if (file.empty()) {
        return std::nullopt;
    }
    const auto bytes = katana::core::readFileBytes(file);
    if (!bytes) {
        return std::nullopt;
    }
    return katana::entity::customisationDigest(*bytes);
}

bool isThere(const fs::path& file)
{
    std::error_code unknown;
    // When it cannot be told, it is taken to be there: the read that follows
    // then says why, and nothing is written over what might be a file.
    return fs::exists(file, unknown) || unknown;
}

// Writes `text` as the whole of `file`: to a file beside it first, which is
// then renamed into its place, so a write that fails part way leaves what
// was there. With `backup`, the file that was there is first COPIED to that
// name (an earlier one of that name is replaced) - copied, not moved, so
// that at no moment is there no `file`: a start between the two steps reads
// the one or the other, never the built-in by accident.
Status writeWhole(const fs::path& file, std::string_view text, const fs::path* backup)
{
    const auto failure = [&file](const std::string& why, const std::error_code& code = {}) {
        return makeError(ErrorCode::FileExportFailure, why,
                         katana::core::pathToUtf8(file) +
                             (code ? ": " + code.message() : std::string{}));
    };
    std::error_code ignored;
    if (file.has_parent_path()) {
        // A per-user folder that nothing has written to yet does not exist.
        // If it cannot be made, opening the file says so.
        fs::create_directories(file.parent_path(), ignored);
    }
    const fs::path temporary = beside(file, ".tmp");
    {
        std::ofstream out(temporary, std::ios::binary | std::ios::trunc);
        out.write(text.data(), static_cast<std::streamsize>(text.size()));
        out.flush();
        if (!out) {
            out.close();
            fs::remove(temporary, ignored);
            return failure("the file cannot be written");
        }
    }
    std::error_code failed;
    if (backup != nullptr && isThere(file)) {
        fs::copy_file(file, *backup, fs::copy_options::overwrite_existing, failed);
        if (failed) {
            fs::remove(temporary, ignored);
            return failure("the file that is there could not be kept beside it, so nothing "
                           "was written",
                           failed);
        }
    }
    fs::rename(temporary, file, failed);
    if (failed) {
        fs::remove(temporary, ignored);
        return failure("the file cannot be written", failed);
    }
    return {};
}

// ---- the report ---------------------------------------------------------------------------

std::string stateRecord(const Document& document)
{
    return customisationStateRecord(customisationSummary(document));
}

std::string trimmedReply(std::string text)
{
    while (!text.empty() && text.back() == '\n') {
        text.pop_back();
    }
    return text;
}

// ---- loading ------------------------------------------------------------------------------

Result<std::string> load(Document& document, const Words& files, LoadMode mode)
{
    if (files.empty()) {
        return usage(kLoadUsage);
    }
    std::vector<fs::path> named;
    named.reserve(files.size());
    for (const std::string& file : files) {
        named.push_back(katana::core::pathFromUtf8(file));
    }
    // A file named twice is read once: read twice, each of its rules would
    // sit in the map twice, since both copies are the load's.
    const DistinctFiles once = distinctCustomisationFiles(named);

    // Every file is read before anything is judged, so that a load is
    // refused once, with all that is wrong with it.
    std::vector<Customisation> loaded;
    std::vector<katana::core::Error> unread;
    for (const fs::path& file : once.files) {
        auto read = readCustomisationFile(file);
        if (!read) {
            unread.push_back(read.error());
        } else {
            loaded.push_back(std::move(read->customisation));
        }
    }
    if (unread.size() == 1) {
        // The reader's own words, whole: which file, and what it is not.
        return unread.front();
    }
    if (!unread.empty()) {
        std::string message = number(unread.size()) + " of the " + number(once.files.size()) +
                              " files were not read, so nothing was loaded";
        for (const katana::core::Error& refusal : unread) {
            message += "\n  " + refusal.describe();
        }
        return makeError(unread.front().code, std::move(message));
    }

    CustomisationMerge merge = mergeCustomisation(document.customisation(), loaded, mode);
    if (!merge.ok()) {
        std::string message = "nothing was loaded, for " + number(merge.problems.size()) +
                              (merge.problems.size() == 1 ? " reason" : " reasons");
        for (const std::string& problem : merge.problems) {
            message += "\n  " + problem;
        }
        return makeError(ErrorCode::InvalidArgument, std::move(message));
    }
    // The merge judged the loads; the install judges the session's own parts
    // (a definition's source set by hand, a name it never read from a file).
    if (auto installed = document.installCustomisation(std::move(merge.merged),
                                                       CustomisationOrigin::Loaded);
        !installed) {
        return installed.error();
    }

    std::string reply;
    for (const fs::path& repeat : once.repeats) {
        reply += "repeated file=" + pathValue(repeat) + "\n";
    }
    for (std::size_t i = 0; i < merge.loads.size(); ++i) {
        const CustomisationLoad& one = merge.loads[i];
        reply += "loaded file=" + pathValue(once.files[i]) + " name=" + recordValue(one.name) +
                 " definitions_added=" + number(one.addedDefinitions.size()) +
                 " definitions_replaced=" + number(one.replacedDefinitions.size()) +
                 " codes_added=" + number(one.addedKeys.size()) +
                 " codes_replaced=" + number(one.replacedKeys.size()) +
                 " colours_added=" + number(one.addedColours.size()) +
                 " colours_replaced=" + number(one.replacedColours.size()) +
                 " linework=" + yesNo(one.linework) + " automation=" + yesNo(one.automation) +
                 "\n";
    }
    if (!merge.removedDefinitions.empty() || !merge.removedKeys.empty()) {
        reply += "removed definitions=" + number(merge.removedDefinitions.size()) +
                 " codes=" + number(merge.removedKeys.size()) + "\n";
        listNames(reply, "removed", "definition", merge.removedDefinitions);
        listNames(reply, "removed", "code", merge.removedKeys);
    }
    // Judged against everything now loaded: a file of codes may name what an
    // earlier load defined.
    if (const Words undefined = undefinedRuleNames(document); !undefined.empty()) {
        reply += "undefined names=" + number(undefined.size()) + "\n";
        listNames(reply, "undefined", "name", undefined);
    }
    return reply + stateRecord(document);
}

// ---- export -------------------------------------------------------------------------------

// One of EXPORT's own words, in any case.
bool isExportWord(const std::string& word)
{
    for (const char* own : {"CODES", "LINESTYLES", "SYMBOLS", "NAME", "ONLY"}) {
        if (is(word, own)) {
            return true;
        }
    }
    return false;
}

Result<std::string> exportTo(const Document& document, const Words& args)
{
    if (args.empty()) {
        return usage(kExportUsage);
    }
    // The first word is the file. One of EXPORT's own words there is a line
    // whose file was left out - CUSTOMISE EXPORT CODES wrote the whole session
    // into a file called CODES - and a file that IS so called is given with
    // its directory, as the family's keyword rule has it.
    if (isExportWord(args.front())) {
        // The usage first and the word last: a front end prints the context
        // in brackets after the message, where it would read as one more
        // option of the usage.
        return makeError(ErrorCode::InvalidArgument,
                         std::string("usage: ") + kExportUsage +
                             ": the file comes first, and this is one of EXPORT's own words; a "
                             "file so called is given with its directory (./" +
                             args.front() + ")",
                         args.front());
    }
    const fs::path file = katana::core::pathFromUtf8(args.front());
    katana::entity::CustomisationWriteOptions options;
    std::optional<std::string> name;
    bool linestyles = false;
    bool symbols = false;
    bool codes = false;
    bool only = false;
    // After ONLY every word is a definition's name - up to NAME, unless NAME
    // was given before it, so that a definition of that name can be listed.
    enum class Reading { Options, Names, Done };
    Reading reading = Reading::Options;
    for (std::size_t at = 1; at < args.size(); ++at) {
        const std::string& word = args[at];
        if (reading == Reading::Done) {
            return usage(kExportUsage);
        }
        if (is(word, "NAME") && !name) {
            if (at + 1 >= args.size()) {
                return usage(kExportUsage);
            }
            name = args[++at];
            reading = reading == Reading::Names ? Reading::Done : reading;
            continue;
        }
        if (reading == Reading::Names) {
            options.only.push_back(word);
        } else if (is(word, "ONLY")) {
            only = true;
            reading = Reading::Names;
        } else if (is(word, "CODES")) {
            codes = true;
        } else if (is(word, "LINESTYLES")) {
            linestyles = true;
        } else if (is(word, "SYMBOLS")) {
            symbols = true;
        } else {
            return usage(kExportUsage);
        }
    }
    if (only && options.only.empty()) {
        return usage(kExportUsage);
    }
    // Which kinds are written. A kind word chooses among them; ONLY chooses
    // among the DEFINITIONS, so with it the codes are written only when CODES
    // says so, and both kinds of definition stay open to the names unless
    // LINESTYLES or SYMBOLS narrows them. (CODES once switched every kind it
    // did not name off, ONLY or not, so CODES ONLY <definition> - the codes
    // and one symbol - was refused for the very symbol it asked for.)
    const bool part = linestyles || symbols || codes || only;
    if (part) {
        options.codes = codes;
    }
    if (linestyles || symbols) {
        options.linestyles = linestyles;
        options.symbols = symbols;
    } else if (codes && !only) {
        // The codes alone.
        options.linestyles = false;
        options.symbols = false;
    }

    if (document.customisationState().origin == CustomisationOrigin::None) {
        return makeError(ErrorCode::InvalidState,
                         "no customisation is loaded, so there is nothing to export");
    }
    Customisation session = document.customisation();
    if (name) {
        session.name = *name;
    }
    if (auto named = katana::entity::validateCustomisationName(session.name); !named) {
        if (!name && session.name.empty()) {
            return makeError(ErrorCode::InvalidState,
                             "the session's customisation has no name to be written under; "
                             "give one: CUSTOMISE EXPORT <file> NAME <name>");
        }
        return makeError(ErrorCode::InvalidArgument, "NAME: " + named.error().message, session.name);
    }
    // After ONLY every word is a definition. One of EXPORT's own words there,
    // naming no definition, is that word put in the wrong place, and "no
    // definition of this name" alone would not say so.
    for (const std::string& word : options.only) {
        if (isExportWord(word) && !session.library.contains(word)) {
            return makeError(ErrorCode::NotFound,
                             "after ONLY every word is a definition's name, and the library has "
                             "no definition of this name; a word of EXPORT's own goes before ONLY",
                             word);
        }
    }

    // The definitions the writer will choose, by the rule it chooses by
    // (entity::CustomisationWriteOptions), counted for the reply.
    const std::set<std::string, std::less<>> chosen(options.only.begin(), options.only.end());
    std::size_t definitions = 0;
    session.library.forEach([&](const katana::entity::LineStyle& definition) {
        const bool kind = definition.symbol ? options.symbols : options.linestyles;
        if (kind && (chosen.empty() || chosen.contains(definition.name))) {
            ++definitions;
        }
    });
    if (part) {
        // A part of the session is a fragment for someone else's, or for
        // this one later, cut down by the one rule everything that writes a
        // part cuts by (customisation_part.hpp) - the two managers' export
        // buttons too, so that the same symbols are the same file whichever
        // wrote them.
        session = customisationPart(std::move(session), options);
    }
    const auto text = katana::entity::customisationToJson(session, options);
    if (!text) {
        return text.error();
    }
    const bool replaced = isThere(file);
    if (auto written = writeWhole(file, *text, nullptr); !written) {
        return written.error();
    }
    return "exported file=" + pathValue(file) + " name=" + recordValue(session.name) +
           " definitions=" + number(definitions) +
           " codes=" + number(options.codes ? session.map.keys().size() : 0) +
           " rules=" + number(options.codes ? session.map.size() : 0) +
           " replaced=" + yesNo(replaced);
}

// ---- the host's verbs ---------------------------------------------------------------------

// The refusal of a verb that needs what the session was not given, by name.
katana::core::Error needsHost(const char* verb, const std::string& what)
{
    return makeError(ErrorCode::InvalidState, std::string("CUSTOMISE ") + verb + " " + what);
}

Result<std::string> resetToBuiltIn(Document& document, const CustomisationVerbContext& context)
{
    if (!context.host) {
        return needsHost("RESET", "needs the program's built-in customisation, and this session "
                                  "was given none");
    }
    const BuiltInCustomisation& builtIn = context.host->builtIn;
    if (builtIn.customisation == nullptr) {
        return needsHost("RESET", builtIn.problem.empty()
                                      ? "needs a built-in customisation, and this program has none"
                                      : "cannot use the built-in customisation: " +
                                            builtIn.problem);
    }
    // Kept only when no kept file would be read in its place at the next
    // start: with one there, the session now differs from it until a KEEP.
    const fs::path& keptFile = context.host->keptFile;
    const bool kept = keptFile.empty() || !isThere(keptFile);
    if (auto installed = installBuiltInCustomisation(document, builtIn, kept); !installed) {
        return installed.error();
    }
    document.setBuiltInCustomisationName(builtIn.customisation->name);
    return "reset name=" + recordValue(document.customisationState().name) +
           countsOf(document) + " kept=" + yesNo(kept);
}

// The path of the kept file, or the refusal that names what is missing.
Result<fs::path> keptFileOf(const char* verb, const CustomisationVerbContext& context)
{
    if (!context.host || context.host->keptFile.empty()) {
        return needsHost(verb, std::string("needs a kept customisation file, and this session "
                                           "has none; the environment variable ") +
                                   kKeptCustomisationVariable + " names one");
    }
    return context.host->keptFile;
}

// What the session is when it is the built-in and nothing more: what
// installing it gives. Asked of the install itself, in a Document of its own,
// so that "is the built-in" can never drift from what a RESET leaves.
std::optional<Customisation> builtInAsInstalled(const BuiltInCustomisation& builtIn)
{
    if (builtIn.customisation == nullptr) {
        return std::nullopt;
    }
    Document scratch;
    if (!installBuiltInCustomisation(scratch, builtIn, true)) {
        return std::nullopt;
    }
    return scratch.customisation();
}

Result<std::string> keep(Document& document, CustomisationVerbContext& context)
{
    const auto file = keptFileOf("KEEP", context);
    if (!file) {
        return file.error();
    }
    if (document.customisationState().origin == CustomisationOrigin::None) {
        return makeError(ErrorCode::InvalidState,
                         "CUSTOMISE KEEP: no customisation is loaded, so there is nothing to keep");
    }
    if (document.customisationState().name.empty()) {
        // Rules or definitions made in an editor with no customisation ever
        // installed. A file needs a name, and the writer's own refusal - a
        // name cannot be empty - would not say how the session gets one.
        return makeError(ErrorCode::InvalidState,
                         "CUSTOMISE KEEP: the session's customisation has no name to be kept "
                         "under; CUSTOMISE EXPORT <file> NAME <name> writes it under one, and "
                         "CUSTOMISE REPLACE <file> then gives the session that name");
    }
    const std::string shown = katana::core::pathToUtf8(*file);

    // What is there now is never written over unseen. Another Katana may
    // have kept its own since this session read the file; a file that does
    // not read may be one its owner means to mend; one from a newer Katana
    // holds what this one cannot write back.
    const bool there = isThere(*file);
    if (there) {
        const auto bytes = katana::core::readFileBytes(*file);
        if (!bytes) {
            return makeError(ErrorCode::InvalidState,
                             "CUSTOMISE KEEP: the kept customisation file cannot be read, so it "
                             "is not written over: " + bytes.error().message,
                             shown);
        }
        if (context.keptDigest != katana::entity::customisationDigest(*bytes)) {
            return makeError(ErrorCode::InvalidState,
                             "CUSTOMISE KEEP: the kept customisation changed on disk since this "
                             "session read it, so it is not written over; CUSTOMISE REVERT reads "
                             "it (CUSTOMISE EXPORT <file> first keeps what this session has)",
                             shown);
        }
        if (const auto read = katana::entity::customisationFromJson(*bytes); !read) {
            const bool newer = read.error().code == ErrorCode::Unsupported;
            return makeError(ErrorCode::InvalidState,
                             std::string("CUSTOMISE KEEP: the kept customisation ") +
                                 (newer ? "was written by a newer Katana"
                                        : "does not read as a Katana customisation") +
                                 ", so it is not written over; move the file away to keep this "
                                 "session's: " + read.error().message,
                             shown);
        }
    }

    const fs::path backup = beside(*file, ".bak");
    const Customisation session = document.customisation();
    if (const auto builtIn = builtInAsInstalled(context.host->builtIn);
        builtIn && session == *builtIn) {
        // Every start gives the built-in when nothing is kept. A copy of it
        // kept all the same would be read in its place at every later start,
        // and so would hide every later edition of the built-in for ever.
        std::error_code failed;
        if (there) {
            std::error_code ignored;
            fs::remove(backup, ignored);
            fs::rename(*file, backup, failed);
            if (failed) {
                return makeError(ErrorCode::FileExportFailure,
                                 "CUSTOMISE KEEP: the kept customisation could not be set aside",
                                 shown + ": " + failed.message());
            }
        }
        context.keptDigest.reset();
        document.setCustomisationKept(true);
        return "kept file=" + pathValue(*file) +
               " written=no reason=the-session-is-the-built-in backup=" +
               (there ? pathValue(backup) : std::string("none"));
    }

    const auto text = katana::entity::customisationToJson(session);
    if (!text) {
        return makeError(text.error().code,
                         "CUSTOMISE KEEP: the session cannot be written: " + text.error().message,
                         text.error().context);
    }
    if (auto written = writeWhole(*file, *text, &backup); !written) {
        return written.error();
    }
    context.keptDigest = katana::entity::customisationDigest(*text);
    document.setCustomisationKept(true);
    return "kept file=" + pathValue(*file) + " written=yes name=" + recordValue(session.name) +
           countsOf(document) + " backup=" + (there ? pathValue(backup) : std::string("none"));
}

Result<std::string> revert(Document& document, CustomisationVerbContext& context)
{
    const auto file = keptFileOf("REVERT", context);
    if (!file) {
        return file.error();
    }
    if (!isThere(*file)) {
        return makeError(ErrorCode::NotFound,
                         "CUSTOMISE REVERT: no customisation is kept, so there is none to go "
                         "back to; CUSTOMISE RESET gives the built-in one",
                         katana::core::pathToUtf8(*file));
    }
    auto read = readCustomisationFile(*file);
    if (!read) {
        return read.error();
    }
    if (auto installed = document.installCustomisation(std::move(read->customisation),
                                                       CustomisationOrigin::Kept, true);
        !installed) {
        return installed.error();
    }
    context.keptDigest = std::move(read->digest);
    return "reverted file=" + pathValue(*file) + " name=" +
           recordValue(document.customisationState().name) + countsOf(document);
}

// ---- removing -----------------------------------------------------------------------------

// The drawing's styles that name a definition, each once and in name order:
// the shared answer (definition_users.hpp) lists a style under its linetype
// AND under its symbol when it names the definition both ways, since it says
// how each names it; this verb counts and cites a style once.
std::vector<std::string> stylesNaming(const DefinitionUsers& users)
{
    std::vector<std::string> styles;
    // Both lists are ascending, as the header promises, so this is the union
    // in the same order.
    std::set_union(users.linetypeStyles.begin(), users.linetypeStyles.end(),
                   users.symbolStyles.begin(), users.symbolStyles.end(),
                   std::back_inserter(styles));
    return styles;
}

// What names a definition is asked of cad::definitionUsers - the function the
// window's definition editor lists a definition's users with, so that what
// REMOVE refuses for is what a person is shown beside Delete: the survey code
// rules that give it as their linestyle or their symbol, the drawing's styles
// that give it as their linetype or their symbol, and the drawing's layers
// that give it as their linetype (an entity with no style of its own is drawn
// with its layer's). A name is a use as the resolver would draw it: the name
// itself, or the earlier name of a definition since renamed.
Result<std::string> removeDefinitions(Document& document, Words names, bool force)
{
    names = distinct(names);
    if (names.empty()) {
        return usage(kRemoveUsage);
    }
    katana::entity::StyleLibrary library = document.styleLibrary();
    std::string unknown;
    for (const std::string& name : names) {
        if (!library.contains(name)) {
            unknown += (unknown.empty() ? "\"" : ", \"") + name + "\"";
        }
    }
    if (!unknown.empty()) {
        return makeError(ErrorCode::NotFound,
                         "CUSTOMISE REMOVE: the library has no definition of that name, so "
                         "nothing was removed",
                         unknown);
    }

    // What each name's record counts: the rules, the styles (each once) and
    // the layers that name it.
    struct Counted {
        std::size_t rules = 0;
        std::size_t styles = 0;
        std::size_t layers = 0;
    };
    std::vector<Counted> uses;
    std::size_t used = 0;
    std::string listing;
    for (const std::string& name : names) {
        const DefinitionUsers users = definitionUsers(document, name);
        const std::vector<std::string> styles = stylesNaming(users);
        uses.push_back(Counted{users.rules.size(), styles.size(), users.layers.size()});
        used += users.empty() ? 0 : 1;
        for (const DefinitionRule& rule : users.rules) {
            listing += "\n  \"" + name + "\": rule #" + number(rule.index) + " " + rule.key + " (" +
                       std::string(katana::entity::toString(rule.section)) + ") names it";
        }
        for (const std::string& style : styles) {
            listing += "\n  \"" + name + "\": the drawing's style \"" + style + "\" names it";
        }
        for (const std::string& layer : users.layers) {
            listing += "\n  \"" + name + "\": the drawing's layer \"" + layer + "\" names it";
        }
    }
    if (used != 0 && !force) {
        return makeError(ErrorCode::InvalidState,
                         "CUSTOMISE REMOVE: " + number(used) + " of the " + number(names.size()) +
                             (names.size() == 1 ? " definition named is" : " definitions named are") +
                             " in use, so nothing was removed; FORCE removes what is used too, "
                             "and what names it then draws plain" + listing);
    }
    for (const std::string& name : names) {
        if (const auto removed = library.remove(name); !removed) {
            return removed.error();
        }
    }
    document.setStyleLibrary(std::move(library));
    std::string reply;
    for (std::size_t i = 0; i < names.size(); ++i) {
        reply += "removed definition=" + recordValue(names[i]) +
                 " rules=" + number(uses[i].rules) +
                 " styles=" + number(uses[i].styles) +
                 " layers=" + number(uses[i].layers) + "\n";
    }
    return trimmedReply(std::move(reply));
}

Result<std::string> removeCodes(Document& document, Words keys)
{
    keys = distinct(keys);
    if (keys.empty()) {
        return usage(kRemoveUsage);
    }
    const std::vector<katana::entity::SurveyRule>& rules = document.surveyMap().rules();
    std::vector<std::size_t> counts(keys.size(), 0);
    katana::entity::SurveyMap kept;
    for (const katana::entity::SurveyRule& rule : rules) {
        const auto found = std::find(keys.begin(), keys.end(), rule.key);
        if (found != keys.end()) {
            ++counts[static_cast<std::size_t>(found - keys.begin())];
            continue;
        }
        // Cannot fail: the rule is in a map, which took it.
        if (auto added = kept.add(rule); !added) {
            return added.error();
        }
    }
    std::string unknown;
    for (std::size_t i = 0; i < keys.size(); ++i) {
        if (counts[i] == 0) {
            unknown += (unknown.empty() ? "\"" : ", \"") + keys[i] + "\"";
        }
    }
    if (!unknown.empty()) {
        // A key is matched as it is written, byte for byte, as a code is.
        return makeError(ErrorCode::NotFound,
                         "CUSTOMISE REMOVE CODE: no rule has that key, so nothing was removed",
                         unknown);
    }
    document.setSurveyMap(std::move(kept));
    std::string reply;
    for (std::size_t i = 0; i < keys.size(); ++i) {
        reply += "removed code=" + recordValue(keys[i]) + " rules=" + number(counts[i]) + "\n";
    }
    return trimmedReply(std::move(reply));
}

Result<std::string> removeFrom(Document& document, const Words& args)
{
    // CODE is the keyword only as the first word, and FORCE only as the last.
    if (!args.empty() && is(args.front(), "CODE")) {
        return removeCodes(document, Words(args.begin() + 1, args.end()));
    }
    const bool force = !args.empty() && is(args.back(), "FORCE");
    return removeDefinitions(
        document, Words(args.begin(), args.end() - (force ? 1 : 0)), force);
}

// ---- settings -----------------------------------------------------------------------------

Result<std::string> setValues(Document& document, const Words& args)
{
    if (args.empty()) {
        return usage(kSetUsage);
    }
    // Everything is read before anything is set, so a line with one bad item
    // sets none of them.
    katana::entity::CustomisationAutomation automation = document.customisationState().automation;
    katana::entity::LineworkCodes linework = document.customisationState().linework;
    std::string reply = "set";
    std::set<std::string> given;
    for (const std::string& word : args) {
        const std::size_t equals = word.find('=');
        if (equals == std::string::npos || equals == 0) {
            return makeError(ErrorCode::InvalidArgument, "a SET item is key=value; " +
                                                             std::string("usage: ") + kSetUsage,
                             word);
        }
        const std::string key = katana::core::lowered(word.substr(0, equals));
        const std::string value = word.substr(equals + 1);
        // A key given twice is two answers to one question. The later one
        // won, and the reply said both back as though both had been set.
        if (!given.insert(key).second) {
            return makeError(ErrorCode::InvalidArgument,
                             "a SET key is given once in a line, and " + key + " is given twice",
                             word);
        }
        if (key == "auto.codes" || key == "auto.linework") {
            const bool on = is(value, "on");
            if (!on && !is(value, "off")) {
                return makeError(ErrorCode::InvalidArgument, key + " is on or off", word);
            }
            (key == "auto.codes" ? automation.codesOnSurveyImport
                                 : automation.lineworkOnSurveyImport) = on;
            reply += " " + key + "=" + (on ? "on" : "off");
            continue;
        }
        bool known = false;
        for (const katana::entity::LineworkCodeMember& member :
             katana::entity::lineworkCodeMembers()) {
            if (key == "linework." + katana::core::lowered(member.name)) {
                // An empty spelling switches the control off.
                linework.*member.spelling = value;
                reply += " " + key + "=" + recordValue(value);
                known = true;
                break;
            }
        }
        if (!known) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string("not a SET key; usage: ") + kSetUsage, word);
        }
    }
    // The codes first: they are the one part that can be refused, and
    // refused they leave the switches unset too.
    if (auto codes = document.setLineworkCodes(std::move(linework)); !codes) {
        return makeError(codes.error().code,
                         "CUSTOMISE SET: the linework codes would be refused, so nothing was "
                         "set: " + codes.error().message,
                         codes.error().context);
    }
    document.setAutomation(automation);
    return reply;
}

} // namespace

CustomisationVerbContext customisationVerbContext(CustomisationHost host)
{
    CustomisationVerbContext context;
    context.keptDigest = digestOfFile(host.keptFile);
    context.host = std::move(host);
    return context;
}

bool isCustomisationVerb(std::string_view verb)
{
    return katana::core::equalsIgnoringCase(verb, "CUSTOMISE");
}

Result<std::string> runCustomisationVerb(Document& document, const std::vector<std::string>& args,
                                         CustomisationVerbContext& context)
{
    if (args.empty()) {
        return trimmedReply(formatCustomisationReply(customisationSummary(document)));
    }
    const std::string& word = args.front();
    const Words rest(args.begin() + 1, args.end());
    // The words that take nothing after them say so, rather than read a
    // second word as a file or ignore it.
    for (const char* alone : {"JSON", "RESET", "KEEP", "REVERT"}) {
        if (is(word, alone) && !rest.empty()) {
            return usage(std::string("CUSTOMISE ") + alone);
        }
    }
    if (is(word, "JSON")) {
        return customisationJson(document);
    }
    if (is(word, "REPLACE")) {
        return load(document, rest, LoadMode::Replace);
    }
    if (is(word, "EXPORT")) {
        return exportTo(document, rest);
    }
    if (is(word, "RESET")) {
        return resetToBuiltIn(document, context);
    }
    if (is(word, "KEEP")) {
        return keep(document, context);
    }
    if (is(word, "REVERT")) {
        return revert(document, context);
    }
    if (is(word, "REMOVE")) {
        return removeFrom(document, rest);
    }
    if (is(word, "SET")) {
        return setValues(document, rest);
    }
    return load(document, args, LoadMode::Merge);
}

std::string customisationVerbHelp()
{
    return R"(CUSTOMISE: the session's customisation - its linestyle and symbol definitions, its survey
code rules, colours, linework codes and automation - read and written as one Katana
customisation file (docs/customisation.md). A keyword is the whole FIRST word, in any case; a
file called as one is given with its directory (./json). CUSTOMIZE is the same verb. A reply
is records, one a line, a file's path written with /.

Report    CUSTOMISE   the counts, then: customisation name= origin=none|builtIn|kept|loaded|
          edited kept=yes|no definitions= codes= rules= colours=  |  source name=
          definitions=yes|no rules=yes|no, one a source  |  automation auto.codes=
          auto.linework=  |  linework linework.start= ... (both as SET takes them)  |  missing
          name=, one a name the open project recorded that is not loaded; then what this
          drawing uses of it
          CUSTOMISE JSON   the same as one JSON object, with the notices, the colours, what
          is wrong with the rules, and the coverage
Load      CUSTOMISE <file> [<file>...]   merge the files into what is loaded: a definition
          takes the place of the one of its name, and the rules a file gives a key in a
          section take the place of that key's rules there
          CUSTOMISE REPLACE <file> [<file>...]   in the place of each KIND the files bring:
          their definitions become the whole library, their rules the whole of the codes; a
          kind they do not bring is kept
          All or nothing: when one file does not read, or one thing in one is refused, none
          is loaded, and every reason is listed. A file named twice is read once (repeated
          file=). A file of another program is not a Katana customisation file.
          loaded file= name= definitions_added= definitions_replaced= codes_added=
          codes_replaced= colours_added= colours_replaced= linework=yes|no automation=yes|no,
          one a file  |  removed definitions= codes= (REPLACE)  |  undefined names= (what the
          rules ask for and nothing defines), each with up to 20 of the names  |  the
          customisation record
Export    CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] [NAME <name>]
          [ONLY <definition>...]   write the session; with a kind word, those kinds alone;
          ONLY, those definitions, of either kind unless LINESTYLES or SYMBOLS says which
          (and no codes unless CODES is said); NAME, under that name.
          A part is written without the linework codes and the automation, so that merging it
          resets nobody's, and lists as its sources only those it holds something of: where
          it is loaded a source is taken at its word. The colours its rules and pens name go
          with it, and every notice. The window's two export buttons write a part the same way.
          A file that is there is written over, and the reply says so:
          exported file= name= definitions= codes= rules= replaced=yes|no
          The file is the first word; a file called as one of EXPORT's own words is given
          with its directory (./CODES). After ONLY every word is a definition; a definition
          called NAME is listed by giving NAME <name> before ONLY
Start     CUSTOMISE RESET   the program's built-in customisation in the place of the session's:
          reset name= definitions= codes= rules= kept=yes|no
          CUSTOMISE KEEP   write the session to the kept file, which the next start reads in
          the place of the built-in; the file that was there is kept beside it as .bak:
          kept file= written=yes name= definitions= codes= rules= backup=
          When the session IS the built-in nothing is written and the kept file is set aside
          (written=no reason=the-session-is-the-built-in), so that a later built-in is not
          hidden by a copy of this one. Refused when the file changed on disk since this
          session read it, does not read, or was written by a newer Katana
          CUSTOMISE REVERT   read the kept file again: reverted file= name= definitions= codes=
          rules=
          These three need what the program was started with; a session given no built-in or
          no kept file refuses each by name
Remove    CUSTOMISE REMOVE <definition>... [FORCE]   refused, listing the rules and the
          drawing's styles and layers that name one, unless FORCE: removed definition=
          rules= styles= layers=
          CUSTOMISE REMOVE CODE <key>...   every rule of each key: removed code= rules=
          (To CHANGE a rule or a definition, merge a file that holds it: CUSTOMISE <file>)
Settings  CUSTOMISE SET <key>=<value>...   auto.codes=on|off and auto.linework=on|off (code a
          survey import's points, then string them); linework.start, .end, .close, .arcstart,
          .arcend, .join and .rectangle=<word>, a control's spelling in a field code (empty:
          that control is off). A key is given once in a line, and one refused item sets
          none: set <key>=<value> ...
Editing a customisation changes the session only: KEEP makes it what the next start gives.)";
}

} // namespace katana::cad
