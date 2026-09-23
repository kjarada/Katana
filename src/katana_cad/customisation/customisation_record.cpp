#include "katana/cad/customisation_record.hpp"

#include <algorithm>
#include <cctype>
#include <set>
#include <system_error>

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
        // A mapfile's rules carry no file name, so a loaded mapfile is taken
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

std::vector<std::string> customisationNotLoaded(const std::vector<std::string>& recorded,
                                                const katana::entity::StyleLibrary& library,
                                                const std::vector<CustomisationSource>& loaded)
{
    const std::vector<std::string> now = customisationRecord(library, loaded);
    std::vector<std::string> missing;
    for (const std::string& name : recorded) {
        if (std::find(now.begin(), now.end(), name) == now.end() &&
            std::find(missing.begin(), missing.end(), name) == missing.end()) {
            missing.push_back(name);
        }
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
    std::erase_if(missingAtOpen, [&](const std::string& name) {
        return std::any_of(load.begin(), load.end(),
                           [&](const CustomisationSource& file) { return file.name == name; });
    });
}

std::vector<std::string> customisationRecordToSave(const std::vector<std::string>& recorded,
                                                   const std::vector<std::string>& missingAtOpen,
                                                   const katana::entity::StyleLibrary& library,
                                                   const std::vector<CustomisationSource>& loaded)
{
    std::vector<std::string> names = customisationRecord(library, loaded);
    for (const std::string& name : recorded) {
        if (std::find(missingAtOpen.begin(), missingAtOpen.end(), name) != missingAtOpen.end() &&
            std::find(names.begin(), names.end(), name) == names.end()) {
            names.push_back(name);
        }
    }
    return names;
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
        } else if (!inQuotes && std::isspace(static_cast<unsigned char>(ch)) != 0) {
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
