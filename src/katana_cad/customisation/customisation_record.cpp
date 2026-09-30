#include "katana/cad/customisation_record.hpp"

#include <algorithm>
#include <array>
#include <set>
#include <system_error>

#include "katana/core/text.hpp"

namespace katana::cad {

void recordCustomisationLoad(std::vector<CustomisationSource>& loaded,
                             const std::vector<CustomisationSource>& load, bool replacedLibrary,
                             bool replacedMap)
{
    std::erase_if(loaded, [&](const CustomisationSource& file) {
        // Replaced: what the earlier files of that kind brought is gone.
        if (file.library ? replacedLibrary : replacedMap) {
            return true;
        }
        // Loaded again: it is now as recent as this load.
        return std::find(load.begin(), load.end(), file) != load.end();
    });
    for (const CustomisationSource& file : load) {
        // Named twice in one load, it was read twice; it is one file.
        if (std::find(loaded.begin(), loaded.end(), file) == loaded.end()) {
            loaded.push_back(file);
        }
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
    for (const CustomisationSource& file : loaded) {
        // A survey code file's rules carry no file name, so a loaded one is taken
        // at its word; a library file is recorded only while it still
        // defines something.
        if ((!file.library || sources.contains(file.name)) &&
            std::find(names.begin(), names.end(), file.name) == names.end()) {
            names.push_back(file.name);
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

std::span<const RenamedSource> builtinRenames()
{
    // Each file's earlier name, hashed by sourceNameHash, and the name it has
    // now. Written out by a script over the two sets of names when the files
    // were renamed; the test of this table checks the names it gives now,
    // which are all it can see.
    static constexpr std::array<RenamedSource, 4> kRenames{{
        {0x851e5f2b3efc2d70ULL, "linestyles.4d"},
        {0x4203f483f25be01eULL, "survey_codes.mapfile"},
        // Its earlier name is a plain one a person's own file may well have.
        {0xc03aa2ccdeee0553ULL, "survey_codes_names.mapfile", false},
        {0x01bf8dab227e48e7ULL, "symbols.4d"},
    }};
    return kRenames;
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

// The rename that answers for `name` in a project's `record`: its own, when the
// earlier name is distinctive or the record also holds a distinctive earlier
// name - the record of the renamed set, not of someone's own file that shares
// a plain name. customisationNotLoaded, noteCustomisationLoaded and
// customisationRecordToSave all ask this, so they cannot come to disagree
// about which recorded names a rename answers.
[[nodiscard]] const RenamedSource* renameAnswering(std::string_view name,
                                                   const std::vector<std::string>& record,
                                                   std::span<const RenamedSource> renamed)
{
    const RenamedSource* rename = renameOf(name, renamed);
    if (rename == nullptr || rename->distinctive) {
        return rename;
    }
    const bool renamedSet = std::any_of(record.begin(), record.end(), [&](const std::string& other) {
        const RenamedSource* beside = renameOf(other, renamed);
        return beside != nullptr && beside->distinctive;
    });
    return renamedSet ? rename : nullptr;
}

} // namespace

std::vector<std::string> customisationNotLoaded(const std::vector<std::string>& recorded,
                                                const katana::entity::StyleLibrary& library,
                                                const std::vector<CustomisationSource>& loaded)
{
    return customisationNotLoaded(recorded, library, loaded, builtinRenames());
}

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
                             const std::vector<CustomisationSource>& load)
{
    noteCustomisationLoaded(missingAtOpen, load, builtinRenames());
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
                                                   const std::vector<CustomisationSource>& loaded)
{
    return customisationRecordToSave(recorded, missingAtOpen, library, loaded, builtinRenames());
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
