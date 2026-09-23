#include "katana/cad/customisation_record.hpp"

#include <algorithm>
#include <set>

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

} // namespace katana::cad
