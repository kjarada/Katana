// The built-in customisations: those compiled into the program, and the seam
// that says what "the built-in" is for one run (customisation_host.hpp).

#include "katana/cad/customisation_host.hpp"

#include <string>
#include <string_view>
#include <utility>

#include "builtin_table.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

namespace {

// A failure is reported, never thrown: a program whose built-in is damaged
// still starts, and says so.
[[nodiscard]] BuiltInCustomisation parseCompiledIn(const BuiltInFile& compiled)
{
    BuiltInCustomisation builtIn;
    auto read = katana::entity::customisationFromJson(compiled.bytes);
    if (!read) {
        builtIn.problem = "the customisation compiled into this program (" +
                          std::string(compiled.file) + ") is not read: " + read.error().describe();
        return builtIn;
    }
    builtIn.customisation =
        std::make_shared<const katana::entity::Customisation>(std::move(*read));
    builtIn.digest = katana::entity::customisationDigest(compiled.bytes);
    return builtIn;
}

// What the build compiled in, as the public type.
[[nodiscard]] std::vector<BuiltInFile> compiledInAsFiles()
{
    std::vector<BuiltInFile> files;
    for (const detail::CompiledInFile& compiled : detail::compiledInFiles()) {
        files.push_back({compiled.file, compiled.bytes});
    }
    return files;
}

// A value the seam may use as a built-in's name: it names no folder, so it
// cannot be a path to a file in one. (A file in the working folder that is
// called like a built-in is given as ./name, as a file called like a keyword
// is in the verbs.)
[[nodiscard]] bool isBareWord(std::string_view seam)
{
    return seam.find_first_of("/\\") == std::string_view::npos;
}

// "\"A\", \"B\"": the names of `named`, for a sentence.
[[nodiscard]] std::string namesOf(std::span<const BuiltInEntry> named)
{
    std::string text;
    for (const BuiltInEntry& entry : named) {
        text += (text.empty() ? "\"" : ", \"") + entry.name + "\"";
    }
    return text;
}

} // namespace

const BuiltInCustomisation& compiledInCustomisation()
{
    // Parsed at the first call and never again: it is some hundreds of
    // definitions, and a session that never asks pays nothing. The default
    // ALONE: an alternate is parsed by builtIns(), when a list or a name asks.
    static const BuiltInCustomisation compiledIn = [] {
        const std::span<const detail::CompiledInFile> files = detail::compiledInFiles();
        return detail::compiledInHasDefault() && !files.empty()
                   ? parseCompiledIn({files.front().file, files.front().bytes})
                   : BuiltInCustomisation{};
    }();
    return compiledIn;
}

const BuiltInEntry* findBuiltIn(std::span<const BuiltInEntry> entries, std::string_view name)
{
    for (const BuiltInEntry& entry : entries) {
        if (katana::core::equalsIgnoringCase(entry.name, name)) {
            return &entry;
        }
    }
    return nullptr;
}

std::string builtInAdviceFor(std::span<const std::string> missing,
                             std::span<const BuiltInEntry> entries)
{
    std::string advice;
    for (const std::string& name : missing) {
        const BuiltInEntry* entry = findBuiltIn(entries, name);
        if (entry == nullptr || entry->builtIn.customisation == nullptr) {
            continue;
        }
        advice += (advice.empty() ? "" : "; ") + std::string("CUSTOMISE BUILTIN ") + entry->name +
                  " makes the built-in \"" + entry->name + "\" the session";
    }
    return advice;
}

std::vector<BuiltInEntry> builtInsFrom(std::span<const BuiltInFile> files,
                                       const BuiltInCustomisation* firstParsed)
{
    std::vector<BuiltInEntry> list;
    for (std::size_t at = 0; at < files.size(); ++at) {
        BuiltInEntry entry;
        entry.source = std::string(files[at].file);
        entry.builtIn =
            at == 0 && firstParsed != nullptr ? *firstParsed : parseCompiledIn(files[at]);
        if (entry.builtIn.customisation != nullptr) {
            entry.name = entry.builtIn.customisation->name;
            entry.description = entry.builtIn.customisation->description;
        } else {
            entry.name = entry.source;
        }
        // One name, one entry: a second file of a name is not offered.
        if (findBuiltIn(list, entry.name) == nullptr) {
            list.push_back(std::move(entry));
        }
    }
    return list;
}

const std::vector<BuiltInEntry>& builtIns()
{
    // The default is the one parse compiledInCustomisation() made.
    static const std::vector<BuiltInEntry> entries = builtInsFrom(
        compiledInAsFiles(), detail::compiledInHasDefault() ? &compiledInCustomisation() : nullptr);
    return entries;
}

BuiltInCustomisation builtInCustomisation()
{
    // THE one place the seam is read. What it means is the function below's.
    // Through core, never getenv: on Windows that gives the ANSI code page's
    // bytes, in which a file named outside the code page is a file named
    // with '?' (core/path_text.hpp).
    const std::string seam = katana::core::environmentVariable(kBuiltInCustomisationVariable);
    // Every alternate is parsed for a name to be found among them, which a
    // start that names a file, or none, or nothing, has no use for: it would
    // pay for megabytes it never reads.
    const bool mayBeAName =
        !seam.empty() && katana::core::lowered(seam) != "none" && isBareWord(seam);
    return builtInCustomisationFor(seam, compiledInCustomisation(),
                                   mayBeAName ? std::span<const BuiltInEntry>(builtIns())
                                              : std::span<const BuiltInEntry>());
}

BuiltInCustomisation builtInCustomisationFor(std::string_view seam,
                                             const BuiltInCustomisation& compiledIn,
                                             std::span<const BuiltInEntry> named)
{
    if (seam.empty()) {
        return compiledIn;
    }
    if (katana::core::lowered(seam) == "none") {
        return {};
    }
    const bool bareWord = isBareWord(seam);
    if (bareWord) {
        if (const BuiltInEntry* entry = findBuiltIn(named, seam)) {
            return entry->builtIn;
        }
    }
    // As text, through core's one conversion: on Windows the seam's value
    // arrives as UTF-8, but elsewhere, and from any other caller, the text
    // may be narrow bytes that are not - and a path built straight from
    // those THROWS at the first of them (path_text.hpp), out of a function
    // that promises to report and never throw.
    const std::filesystem::path path = katana::core::pathFromUtf8(seam);
    auto file = readCustomisationFile(path);
    if (!file) {
        BuiltInCustomisation none;
        none.problem = std::string("the file ") + kBuiltInCustomisationVariable +
                       " names as the built-in customisation is not read: " +
                       file.error().describe();
        // A word that is no built-in's name is most likely a name mistyped,
        // and the file it was read as says only that it is not there.
        if (bareWord && !named.empty()) {
            none.problem += "; it is no built-in customisation of this program either (it has " +
                            namesOf(named) + "; a file of that name in this folder is ./" +
                            std::string(seam) + ")";
        }
        return none;
    }
    BuiltInCustomisation builtIn;
    builtIn.customisation =
        std::make_shared<const katana::entity::Customisation>(std::move(file->customisation));
    builtIn.digest = std::move(file->digest);
    return builtIn;
}

} // namespace katana::cad
