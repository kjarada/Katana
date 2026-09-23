// Loading a customisation on top of the one already loaded (audit QT-21).
//
// See customisation.hpp for what Merge and Replace mean. The one decision that
// is not obvious is what "the same rule" is in a mapfile, where rules have no
// names: here it is a KEY IN A SECTION. A mapfile says one aspect of a code per
// section - where it goes (map_data), its symbol (vertex_symbol_data), its
// attributes - so a personal mapfile that gives `WM*` a new colour replaces the
// `WM*` map_data rules and leaves the `WM*` symbol alone. Appending instead
// would do nothing at all: among rules of one key the EARLIER wins a field
// (entity::SurveyMap::add), so the rules already loaded would outrank every
// rule the person had just loaded.
//
// WHERE the loaded rules go matters, but only among rules of their own key: a
// code matches rules most specific first, and two DIFFERENT keys of equal
// specificity cannot both match one code (two exact keys differ, and two
// prefixes of one length that both begin a code are the same prefix). Among
// rules of one key it matters in EVERY section, not only the replaced one,
// because some fields are filled by more than one section: a comment by any,
// the string's attributes by pipe_data and string_attribute_data, each
// vertex's by vertex_pipe_data and vertex_attribute_data. So every rule the
// load gives a key goes in AHEAD of every current rule of that key it leaves
// standing - at the first of them - or a current pipe_data `*` DepthLocation
// would outrank the one the person had just loaded in string_attribute_data.
// A key the current map does not have follows, in the order it was loaded.

#include "katana/archive12d/customisation.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <string>
#include <utility>
#include <vector>

namespace katana::archive12d {

namespace {

using katana::entity::LineStyle;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

using RuleGroup = std::pair<SurveySection, std::string>;

[[nodiscard]] RuleGroup groupOf(const SurveyRule& rule) { return {rule.section, rule.key}; }

[[nodiscard]] std::string nameOf(const std::filesystem::path& path)
{
    const std::u8string name = path.filename().u8string();
    return {reinterpret_cast<const char*>(name.data()), name.size()};
}

class Merger {
  public:
    Merger(const katana::entity::StyleLibrary& currentLibrary,
           const katana::entity::SurveyMap& currentMap, const Customisation& loaded, LoadMode mode)
        : currentLibrary_(currentLibrary), currentMap_(currentMap), loaded_(loaded), mode_(mode)
    {
        // The report lists the files in the order they were loaded, including
        // one that turned out to change nothing: "loaded, and it added
        // nothing" is an answer a person wants.
        for (const LoadedFile& file : loaded_.files) {
            fileFor(file.kind, nameOf(file.path));
        }
    }

    CustomisationMerge run()
    {
        mergeLibrary();
        mergeMap();
        return std::move(out_);
    }

  private:
    FileMerge& fileFor(CustomisationFile kind, const std::string& name)
    {
        const auto found = std::find_if(out_.files.begin(), out_.files.end(),
                                        [&](const FileMerge& file) {
                                            return file.kind == kind && file.name == name;
                                        });
        if (found != out_.files.end()) {
            return *found;
        }
        return out_.files.emplace_back(FileMerge{name, kind, {}, {}});
    }

    void mergeLibrary()
    {
        out_.libraryLoaded = !loaded_.library.empty();
        if (!out_.libraryLoaded) {
            // A load of a mapfile alone. Installing its empty library would
            // make every linestyle and symbol stop drawing (QT-21).
            out_.library = currentLibrary_;
            return;
        }
        out_.library = mode_ == LoadMode::Merge ? currentLibrary_ : katana::entity::StyleLibrary{};
        loaded_.library.forEach([this](const LineStyle& definition) {
            // A definition belongs to the file it was read from, which it
            // carries; two loaded files defining one name leave the later's
            // definition, and so the later file is the one that brought it.
            FileMerge& file = fileFor(CustomisationFile::StyleLibrary, definition.source);
            (currentLibrary_.contains(definition.name) ? file.replaced : file.added)
                .push_back(definition.name);
            // Cannot fail: the definition was validated when it entered the
            // loaded library, which is a table that refuses an invalid one.
            // Asserted rather than assumed, so that if that ever changes the
            // definition is reported instead of silently missing.
            if (auto installed = katana::entity::addOrReplace(out_.library, definition); !installed) {
                out_.problems.push_back(installed.error().describe());
            }
        });
        if (mode_ == LoadMode::Replace) {
            currentLibrary_.forEach([this](const LineStyle& definition) {
                if (!loaded_.library.contains(definition.name)) {
                    out_.removedDefinitions.push_back(definition.name);
                }
            });
        }
    }

    // Which loaded file each rule of loaded_.map came from: readCustomisationInto
    // appends each mapfile's rules in order and records how many it added.
    [[nodiscard]] std::vector<std::string> ruleOwners() const
    {
        std::vector<std::string> owners;
        owners.reserve(loaded_.map.size());
        for (const LoadedFile& file : loaded_.files) {
            if (file.kind != CustomisationFile::MapFile) {
                continue;
            }
            for (std::size_t i = 0; i < file.read && owners.size() < loaded_.map.size(); ++i) {
                owners.push_back(nameOf(file.path));
            }
        }
        // Rules no file accounts for - a map built in code, not loaded - are
        // reported under no name rather than credited to the last file.
        owners.resize(loaded_.map.size());
        return owners;
    }

    void add(katana::entity::SurveyMap& map, const SurveyRule& rule)
    {
        // As for definitions: every rule came from a map that validated it.
        if (auto status = map.add(rule); !status) {
            out_.problems.push_back(status.error().describe());
        }
    }

    void mergeMap()
    {
        out_.mapLoaded = !loaded_.map.empty();
        if (!out_.mapLoaded) {
            // A load of a symbol file alone. Installing its empty map would
            // leave Apply Survey Codes with nothing to apply (QT-21).
            out_.map = currentMap_;
            return;
        }

        std::set<RuleGroup> current;
        for (const SurveyRule& rule : currentMap_.rules()) {
            current.insert(groupOf(rule));
        }
        std::set<RuleGroup> loaded;
        const std::vector<std::string> owners = ruleOwners();
        std::set<std::pair<std::string, RuleGroup>> reported;
        const auto& rules = loaded_.map.rules();
        for (std::size_t i = 0; i < rules.size(); ++i) {
            const RuleGroup group = groupOf(rules[i]);
            loaded.insert(group);
            // Once per file, key and section: a file giving `WM*` three
            // map_data rules replaced one thing, not three.
            if (reported.insert({owners[i], group}).second) {
                FileMerge& file = fileFor(CustomisationFile::MapFile, owners[i]);
                (current.contains(group) ? file.replaced : file.added).push_back(group.second);
            }
        }

        if (mode_ == LoadMode::Replace) {
            out_.map = loaded_.map;
            const std::vector<std::string> kept = loaded_.map.keys();
            for (const std::string& key : currentMap_.keys()) {
                if (!std::binary_search(kept.begin(), kept.end(), key)) {
                    out_.removedKeys.push_back(key);
                }
            }
            return;
        }

        // Merge: at the first current rule of a key the load gives rules
        // for, ALL the loaded rules of that key go in, in their order; after
        // them, the current rules of that key in sections the load did not
        // give it. The current rules of a loaded (section, key) group are
        // dropped - they are what was replaced.
        std::map<std::string, std::vector<const SurveyRule*>> loadedByKey;
        for (const SurveyRule& rule : rules) {
            loadedByKey[rule.key].push_back(&rule);
        }
        katana::entity::SurveyMap merged;
        std::set<std::string> placed;
        for (const SurveyRule& rule : currentMap_.rules()) {
            if (const auto found = loadedByKey.find(rule.key);
                found != loadedByKey.end() && placed.insert(rule.key).second) {
                for (const SurveyRule* loadedRule : found->second) {
                    add(merged, *loadedRule);
                }
            }
            if (!loaded.contains(groupOf(rule))) {
                add(merged, rule);
            }
        }
        for (const SurveyRule& rule : rules) {
            if (!placed.contains(rule.key)) {
                add(merged, rule);
            }
        }
        out_.map = std::move(merged);
    }

    const katana::entity::StyleLibrary& currentLibrary_;
    const katana::entity::SurveyMap& currentMap_;
    const Customisation& loaded_;
    LoadMode mode_;
    CustomisationMerge out_{};
};

} // namespace

const char* toString(LoadMode mode) { return mode == LoadMode::Merge ? "merge" : "replace"; }

CustomisationMerge mergeCustomisation(const katana::entity::StyleLibrary& currentLibrary,
                                      const katana::entity::SurveyMap& currentMap,
                                      const Customisation& loaded, LoadMode mode)
{
    return Merger(currentLibrary, currentMap, loaded, mode).run();
}

} // namespace katana::archive12d
