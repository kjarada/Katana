#include "katana/cad/customisation_record.hpp"

#include <algorithm>
#include <array>
#include <set>
#include <system_error>

#include "katana/cad/customisation_host.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

void recordCustomisationLoad(std::vector<CustomisationSource>& loaded,
                             const std::vector<CustomisationSource>& load,
                             bool replacedDefinitions, bool replacedRules)
{
    // Replaced: what the earlier sources brought of that kind is gone. One
    // that brought the other kind as well still brings that; one that now
    // brings neither goes. A source that never brought either - a table of
    // colours - is no kind a Replace takes the place of, and stays.
    std::vector<CustomisationSource> standing;
    standing.reserve(loaded.size() + load.size());
    for (CustomisationSource& source : loaded) {
        const bool brought = source.definitions || source.rules;
        source.definitions = source.definitions && !replacedDefinitions;
        source.rules = source.rules && !replacedRules;
        if (!brought || source.definitions || source.rules) {
            standing.push_back(std::move(source));
        }
    }
    loaded = std::move(standing);
    for (const CustomisationSource& source : load) {
        CustomisationSource entry = source;
        // Loaded again - or named twice in this load, which read it twice and
        // is still one source: it is now as recent as this load, and what it
        // brought before and still brings is its own as well.
        const auto earlier =
            std::find_if(loaded.begin(), loaded.end(),
                         [&](const CustomisationSource& one) { return one.name == source.name; });
        if (earlier != loaded.end()) {
            entry.definitions = entry.definitions || earlier->definitions;
            entry.rules = entry.rules || earlier->rules;
            if (entry.notice.empty()) {
                entry.notice = std::move(earlier->notice);
            }
            loaded.erase(earlier);
        }
        loaded.push_back(std::move(entry));
    }
}

std::vector<std::string> customisationRecord(const katana::entity::StyleLibrary& library,
                                             const std::vector<CustomisationSource>& loaded)
{
    std::set<std::string> sources;
    library.forEach([&](const katana::entity::LineStyle& definition) {
        if (!definition.source.empty()) {
            sources.insert(definition.source);
        }
    });
    std::vector<std::string> names;
    for (const CustomisationSource& source : loaded) {
        // Rules carry no source name, so a source that brought any is taken at
        // its word, and so is one that brought neither kind (its colours are
        // still what names are drawn in); one that brought definitions alone
        // is recorded only while it still defines something.
        const bool definitionsAlone = source.definitions && !source.rules;
        if ((!definitionsAlone || sources.contains(source.name)) &&
            std::find(names.begin(), names.end(), source.name) == names.end()) {
            names.push_back(source.name);
        }
    }
    // Definitions from a file no load here accounts for - the library was
    // given some other way - are still what the drawing is drawn with.
    for (const std::string& source : sources) {
        if (std::find(names.begin(), names.end(), source) == names.end()) {
            names.push_back(source);
        }
    }
    return names;
}

std::uint64_t sourceNameHash(std::string_view name)
{
    std::uint64_t hash = 0xcbf29ce484222325ULL; // the FNV-1a 64-bit offset basis
    for (const char ch : name) {
        hash ^= static_cast<unsigned char>(ch);
        hash *= 0x100000001b3ULL; // the FNV 64-bit prime
    }
    return hash;
}

std::vector<RenamedSource> builtinRenames(std::string_view builtIn)
{
    // The four files' first names, hashed by sourceNameHash, in load order.
    // Written out by a script over the two sets of names when the files were
    // given general ones: nothing here can spell them.
    struct Former {
        std::uint64_t hash;
        bool distinctive;
    };
    static constexpr std::array<Former, 4> kFirstNames{{
        {0x851e5f2b3efc2d70ULL, true},  // the linestyle library
        {0x4203f483f25be01eULL, true},  // the first survey code file
        // A plain name, one a person's own file may well have.
        {0xc03aa2ccdeee0553ULL, false}, // the second survey code file
        {0x01bf8dab227e48e7ULL, true},  // the symbol library
    }};
    // The names the four files had next, until the built-in became one
    // customisation: general names, so every one of them is plain.
    static constexpr std::array<std::string_view, 4> kGeneralNames{
        "linestyles.4d", "survey_codes.mapfile", "survey_codes_names.mapfile", "symbols.4d"};

    std::vector<RenamedSource> renames;
    renames.reserve(kFirstNames.size() + kGeneralNames.size());
    for (const Former& former : kFirstNames) {
        renames.push_back({former.hash, std::string(builtIn), former.distinctive, 0});
    }
    for (const std::string_view name : kGeneralNames) {
        renames.push_back({sourceNameHash(name), std::string(builtIn), false, 1});
    }
    return renames;
}

std::vector<RenamedSource> builtinRenames()
{
    const BuiltInCustomisation& compiledIn = compiledInCustomisation();
    return builtinRenames(compiledIn.customisation ? compiledIn.customisation->name
                                                   : std::string());
}

namespace {

// The rename of `renamed` whose earlier name `name` is, if any.
[[nodiscard]] const RenamedSource* renameOf(std::string_view name,
                                            std::span<const RenamedSource> renamed)
{
    const std::uint64_t hash = sourceNameHash(name);
    const auto found = std::find_if(renamed.begin(), renamed.end(),
                                    [&](const RenamedSource& r) { return r.formerName == hash; });
    return found == renamed.end() ? nullptr : &*found;
}

// The rename that answers for `name` in a project's `record`: its own, when
// the earlier name is distinctive or the record also holds ANOTHER earlier
// name of the same set - the record of the renamed set, not of someone's own
// file that shares a plain name. Nothing, when the rename has no name to
// answer with. customisationNotLoaded, noteCustomisationLoaded and
// customisationRecordToSave all ask this, so they cannot come to disagree
// about which recorded names a rename answers.
[[nodiscard]] const RenamedSource* renameAnswering(std::string_view name,
                                                   const std::vector<std::string>& record,
                                                   std::span<const RenamedSource> renamed)
{
    const RenamedSource* rename = renameOf(name, renamed);
    if (rename == nullptr || rename->now.empty()) {
        return nullptr;
    }
    if (rename->distinctive) {
        return rename;
    }
    const bool renamedSet = std::any_of(record.begin(), record.end(), [&](const std::string& other) {
        // The name itself is no company: recorded twice, it is still alone.
        if (other == name) {
            return false;
        }
        const RenamedSource* beside = renameOf(other, renamed);
        return beside != nullptr && beside->set == rename->set;
    });
    return renamedSet ? rename : nullptr;
}

} // namespace

std::vector<std::string> customisationNotLoaded(const std::vector<std::string>& recorded,
                                                const katana::entity::StyleLibrary& library,
                                                const std::vector<CustomisationSource>& loaded,
                                                std::span<const RenamedSource> renamed)
{
    const std::vector<std::string> now = customisationRecord(library, loaded);
    const auto isNow = [&](std::string_view name) {
        return std::find(now.begin(), now.end(), name) != now.end();
    };
    std::vector<std::string> missing;
    for (const std::string& name : recorded) {
        if (isNow(name) || std::find(missing.begin(), missing.end(), name) != missing.end()) {
            continue;
        }
        // Recorded under a name its file no longer has: answered by the file
        // that has its place, when that file is loaded now.
        if (const RenamedSource* rename = renameAnswering(name, recorded, renamed);
            rename != nullptr && isNow(rename->now)) {
            continue;
        }
        missing.push_back(name);
    }
    return missing;
}

DistinctFiles distinctCustomisationFiles(const std::vector<std::filesystem::path>& named)
{
    DistinctFiles distinct;
    for (const std::filesystem::path& path : named) {
        std::error_code error;
        std::filesystem::path file = std::filesystem::absolute(path, error).lexically_normal();
        if (error) {
            // Unresolvable here; the read that follows says why.
            file = path.lexically_normal();
        }
        const bool repeat = std::any_of(
            distinct.files.begin(), distinct.files.end(), [&](const std::filesystem::path& seen) {
                if (seen == file) {
                    return true;
                }
                // equivalent() reports an error, not false, for a file that
                // does not exist; either way they are not known to be one.
                std::error_code unknown;
                return std::filesystem::equivalent(seen, file, unknown) && !unknown;
            });
        if (repeat) {
            distinct.repeats.push_back(path);
        } else {
            distinct.files.push_back(std::move(file));
        }
    }
    return distinct;
}

void noteCustomisationLoaded(std::vector<std::string>& missingAtOpen,
                             const std::vector<CustomisationSource>& load,
                             std::span<const RenamedSource> renamed)
{
    // Judged against the list as the load found it, not as it shrinks.
    const std::vector<std::string> before = missingAtOpen;
    std::erase_if(missingAtOpen, [&](const std::string& name) {
        const RenamedSource* rename = renameAnswering(name, before, renamed);
        return std::any_of(load.begin(), load.end(), [&](const CustomisationSource& file) {
            return file.name == name || (rename != nullptr && file.name == rename->now);
        });
    });
}

std::vector<std::string> customisationRecordToSave(const std::vector<std::string>& recorded,
                                                   const std::vector<std::string>& missingAtOpen,
                                                   const katana::entity::StyleLibrary& library,
                                                   const std::vector<CustomisationSource>& loaded,
                                                   std::span<const RenamedSource> renamed)
{
    std::vector<std::string> names = customisationRecord(library, loaded);
    // What the whole record still lacks: a missing earlier name that
    // noteCustomisationLoaded could not clear, seeing only the missing names,
    // is judged here against the record, as the open judged it.
    const std::vector<std::string> stillMissing =
        customisationNotLoaded(recorded, library, loaded, renamed);
    for (const std::string& name : recorded) {
        if (std::find(missingAtOpen.begin(), missingAtOpen.end(), name) != missingAtOpen.end() &&
            std::find(stillMissing.begin(), stillMissing.end(), name) != stillMissing.end() &&
            std::find(names.begin(), names.end(), name) == names.end()) {
            names.push_back(name);
        }
    }
    return names;
}

std::span<const RenamedDefinition> builtinDefinitionRenames()
{
    // Each definition's earlier name, hashed by sourceNameHash, and the name it
    // has now: the earlier name with the publisher's leading word taken off.
    // Written out by a script over the definitions before and after the
    // rename; the tests check the names given now against the built-in
    // customisation itself.
    static constexpr std::array<RenamedDefinition, 29> kRenames{{
        {0x063e2ef59dcfd3b6ULL, "Accepted For Construction"},
        {0xcc0997296f480f64ULL, "Horizontal Scale 1 to 100"},
        {0xfc5b2f6817720bbcULL, "Horizontal Scale 1 to 1000"},
        {0x40fd19dfd6c99ee4ULL, "Horizontal Scale 1 to 10000"},
        {0xe59988297da41305ULL, "Horizontal Scale 1 to 200"},
        {0xc7f593807dcca30fULL, "Horizontal Scale 1 to 2000"},
        {0xe5888a297d95a338ULL, "Horizontal Scale 1 to 250"},
        {0x9ca5cc8065440698ULL, "Horizontal Scale 1 to 2500"},
        {0x0550f36d8ea142f8ULL, "Horizontal Scale 1 to 50"},
        {0xa9d06b295c0479d8ULL, "Horizontal Scale 1 to 500"},
        {0x919fff475b9b2538ULL, "Horizontal Scale 1 to 5000"},
        {0x44fd410a36d5505eULL, "North Point no whiteout"},
        {0xafb836a2812948fbULL, "North Point with whiteout"},
        {0x6ab0f4901d43b624ULL, "Not For Construction"},
        {0x3c35747b30ec8efcULL, "SM Basin Label"},
        {0x425550dc0f20e0eeULL, "SM Pit"},
        {0x56e0127bc52dbcfbULL, "SURVEY - FU"},
        {0x56e00f7bc52db7e2ULL, "SURVEY - FZ"},
        {0x56f45c7bc53ee3c3ULL, "SURVEY - HO"},
        {0x56f44f7bc53ecdacULL, "SURVEY - HZ"},
        {0x8d38f77d0bc7ff92ULL, "Site of Work"},
        {0x23dd48a9e9ff2bbeULL, "Vertical Scale 1 to 100"},
        {0xf02e06b89c97024aULL, "Vertical Scale 1 to 1000"},
        {0xc98cd081dca080f1ULL, "Vertical Scale 1 to 20"},
        {0x1ac70da9e4bac7f3ULL, "Vertical Scale 1 to 200"},
        {0xc98cd381dca0860aULL, "Vertical Scale 1 to 25"},
        {0x1ad19fa9e4c4148eULL, "Vertical Scale 1 to 250"},
        {0xc99dce81dcaef0beULL, "Vertical Scale 1 to 50"},
        {0x461674a9fd42c14aULL, "Vertical Scale 1 to 500"},
    }};
    return kRenames;
}

std::string_view definitionNameNow(std::string_view name)
{
    return definitionNameNow(name, builtinDefinitionRenames());
}

std::string_view definitionNameNow(std::string_view name,
                                   std::span<const RenamedDefinition> renamed)
{
    if (name.empty()) {
        return {};
    }
    const std::uint64_t hash = sourceNameHash(name);
    const auto found = std::find_if(renamed.begin(), renamed.end(),
                                    [&](const RenamedDefinition& r) { return r.formerName == hash; });
    return found == renamed.end() ? std::string_view{} : found->now;
}

bool typedSaveHasDestination(std::string_view line, bool hasProject)
{
    // CommandInterpreter's word rule (its tokenize), counted rather than
    // kept: blanks split, double quotes group and "" is a word.
    std::size_t words = 0;
    bool inQuotes = false;
    bool inWord = false;
    for (const char ch : line) {
        if (ch == '"') {
            inQuotes = !inQuotes;
            inWord = true;
        } else if (!inQuotes && katana::core::isAsciiSpace(ch)) {
            if (inWord) {
                ++words;
                inWord = false;
            }
        } else {
            inWord = true;
        }
    }
    if (inQuotes) {
        return false; // the interpreter refuses the line
    }
    if (inWord) {
        ++words;
    }
    // The verb is the first word.
    return words == 2 || (words == 1 && hasProject);
}

} // namespace katana::cad
