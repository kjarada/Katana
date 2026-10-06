// Loading customisations on top of the one a session has: see
// customisation_merge.hpp for the rule and why it is the rule.

#include "katana/cad/customisation_merge.hpp"

#include <algorithm>
#include <cstddef>
#include <map>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/customisation_record.hpp"
#include "katana/cad/customisation_state.hpp"

namespace katana::cad {

namespace {

using katana::entity::ColourTable;
using katana::entity::Customisation;
using katana::entity::LineStyle;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

using RuleGroup = std::pair<SurveySection, std::string>;

[[nodiscard]] RuleGroup groupOf(const SurveyRule& rule) { return {rule.section, rule.key}; }

class Merger {
  public:
    Merger(const Customisation& current, std::span<const Customisation> loaded, LoadMode mode)
        : current_(current), loaded_(loaded), mode_(mode)
    {
    }

    CustomisationMerge run()
    {
        if (loaded_.empty()) {
            // Nothing was loaded, so nothing changes - not even the session's
            // list of sources, which a load would otherwise have completed.
            out_.merged = current_;
            return std::move(out_);
        }
        checkLoads();
        mergeLibrary();
        mergeMap();
        mergeColours();
        mergeSettings();
        mergeHeader();
        if (!out_.problems.empty()) {
            // All or nothing: half a load is not something to install.
            out_.merged = current_;
        }
        return std::move(out_);
    }

  private:
    // How a problem names the customisation it is about.
    [[nodiscard]] std::string labelOf(std::size_t index) const
    {
        const std::string& name = loaded_[index].name;
        if (katana::entity::validateCustomisationName(name).ok()) {
            return "\"" + name + "\"";
        }
        return "customisation " + std::to_string(index + 1) + " of this load";
    }

    void problem(std::size_t index, const std::string& what)
    {
        out_.problems.push_back(labelOf(index) + ": " + what);
    }

    // One entry per loaded customisation, in order, and everything about one
    // that would make the session unfit to be kept or recorded - the very
    // list Document::installCustomisation refuses on (customisationFaults),
    // so that what a load is told here and what an install would then refuse
    // cannot come to differ. A customisation read from a file has passed all
    // of this already; one built in code has not.
    void checkLoads()
    {
        for (std::size_t i = 0; i < loaded_.size(); ++i) {
            const Customisation& load = loaded_[i];
            out_.loads.push_back(
                CustomisationLoad{load.name, !load.library.empty(), !load.map.empty()});
            for (const katana::core::Error& fault : customisationFaults(load)) {
                problem(i, fault.context.empty() ? fault.message
                                                 : fault.message + " [" + fault.context + "]");
            }
        }
    }

    void mergeLibrary()
    {
        // Two loaded customisations defining one name leave the later's
        // definition, and so the later is the one that brought it.
        std::map<std::string, std::size_t, std::less<>> owner;
        for (std::size_t i = 0; i < loaded_.size(); ++i) {
            loaded_[i].library.forEach(
                [&](const LineStyle& definition) { owner[definition.name] = i; });
        }
        out_.definitionsLoaded = !owner.empty();
        if (!out_.definitionsLoaded) {
            // A load of survey codes alone. Installing its empty library
            // would make every linestyle and symbol stop drawing.
            out_.merged.library = current_.library;
            return;
        }
        out_.merged.library =
            mode_ == LoadMode::Merge ? current_.library : katana::entity::StyleLibrary{};
        for (const auto& [name, index] : owner) {
            const LineStyle* definition = loaded_[index].library.find(name);
            CustomisationLoad& load = out_.loads[index];
            (current_.library.contains(name) ? load.replacedDefinitions : load.addedDefinitions)
                .push_back(name);
            // Cannot fail: the definition was validated when it entered the
            // loaded library, a table that refuses an invalid one. Reported
            // rather than assumed, so that if that ever changes the definition
            // stops the load instead of going missing without a word.
            if (auto installed = katana::entity::addOrReplace(out_.merged.library, *definition);
                !installed) {
                problem(index, installed.error().describe());
            }
        }
        if (mode_ == LoadMode::Replace) {
            current_.library.forEach([&](const LineStyle& definition) {
                if (!owner.contains(definition.name)) {
                    out_.removedDefinitions.push_back(definition.name);
                }
            });
        }
    }

    void add(katana::entity::SurveyMap& map, const SurveyRule& rule, std::size_t index)
    {
        // As for definitions: every rule came from a map that validated it.
        if (auto status = map.add(rule); !status) {
            problem(index, status.error().describe());
        }
    }

    void mergeMap()
    {
        // The loaded rules in the order they were loaded, each with the
        // customisation it came from.
        struct LoadedRule {
            const SurveyRule* rule;
            std::size_t load;
        };
        std::vector<LoadedRule> rules;
        for (std::size_t i = 0; i < loaded_.size(); ++i) {
            for (const SurveyRule& rule : loaded_[i].map.rules()) {
                rules.push_back({&rule, i});
            }
        }
        out_.rulesLoaded = !rules.empty();
        if (!out_.rulesLoaded) {
            // A load of symbols alone. Installing its empty map would leave
            // nothing to code a survey with.
            out_.merged.map = current_.map;
            return;
        }

        std::set<RuleGroup> current;
        for (const SurveyRule& rule : current_.map.rules()) {
            current.insert(groupOf(rule));
        }
        std::set<RuleGroup> loaded;
        std::set<std::pair<std::size_t, RuleGroup>> reported;
        for (const LoadedRule& entry : rules) {
            const RuleGroup group = groupOf(*entry.rule);
            loaded.insert(group);
            // Once per customisation, key and section: one giving `WM*` three
            // feature rules replaced one thing, not three.
            if (reported.insert({entry.load, group}).second) {
                CustomisationLoad& load = out_.loads[entry.load];
                (current.contains(group) ? load.replacedKeys : load.addedKeys)
                    .push_back(group.second);
            }
        }

        if (mode_ == LoadMode::Replace) {
            std::set<std::string, std::less<>> kept;
            for (const LoadedRule& entry : rules) {
                add(out_.merged.map, *entry.rule, entry.load);
                kept.insert(entry.rule->key);
            }
            for (const std::string& key : current_.map.keys()) {
                if (!kept.contains(key)) {
                    out_.removedKeys.push_back(key);
                }
            }
            return;
        }

        // Merge: at the first current rule of a key the load gives rules for,
        // ALL the loaded rules of that key go in, in their order; after them,
        // the current rules of that key in sections the load did not give it.
        // The current rules of a loaded (section, key) group are dropped -
        // they are what was replaced.
        std::map<std::string, std::vector<const LoadedRule*>, std::less<>> loadedByKey;
        for (const LoadedRule& entry : rules) {
            loadedByKey[entry.rule->key].push_back(&entry);
        }
        std::set<std::string, std::less<>> placed;
        for (const SurveyRule& rule : current_.map.rules()) {
            if (const auto found = loadedByKey.find(rule.key);
                found != loadedByKey.end() && placed.insert(rule.key).second) {
                for (const LoadedRule* entry : found->second) {
                    add(out_.merged.map, *entry->rule, entry->load);
                }
            }
            if (!loaded.contains(groupOf(rule))) {
                // A current rule is the session's own; were it refused, the
                // fault would lie with no customisation of this load.
                if (auto status = out_.merged.map.add(rule); !status) {
                    out_.problems.push_back("the session's own rules: " +
                                            status.error().describe());
                }
            }
        }
        for (const LoadedRule& entry : rules) {
            if (!placed.contains(entry.rule->key)) {
                add(out_.merged.map, *entry.rule, entry.load);
            }
        }
    }

    void mergeColours()
    {
        // By the fold, which is a colour name's identity: the loaded "SUI Gas"
        // takes the place of the session's "sui_gas".
        std::map<std::string, ColourTable::Entry> byFold;
        for (ColourTable::Entry& entry : current_.colours.entries()) {
            std::string fold = katana::entity::foldColourName(entry.name);
            byFold.insert_or_assign(std::move(fold), std::move(entry));
        }
        // Whose entry each fold is, for a problem to name: none for the
        // session's own.
        std::map<std::string, std::size_t> owner;
        for (std::size_t i = 0; i < loaded_.size(); ++i) {
            for (ColourTable::Entry& entry : loaded_[i].colours.entries()) {
                CustomisationLoad& load = out_.loads[i];
                (current_.colours.find(entry.name) ? load.replacedColours : load.addedColours)
                    .push_back(entry.name);
                std::string fold = katana::entity::foldColourName(entry.name);
                owner.insert_or_assign(fold, i);
                byFold.insert_or_assign(std::move(fold), std::move(entry));
            }
        }
        for (const auto& [fold, entry] : byFold) {
            // Cannot fail, each entry having come out of a table that took
            // it and no two sharing a fold; reported if it ever does.
            if (auto status = out_.merged.colours.add(entry.name, entry.colour); !status) {
                if (const auto found = owner.find(fold); found != owner.end()) {
                    problem(found->second, status.error().describe());
                } else {
                    out_.problems.push_back("the session's own colours: " +
                                            status.error().describe());
                }
            }
        }
    }

    void mergeSettings()
    {
        out_.merged.linework = current_.linework;
        out_.merged.automation = current_.automation;
        for (std::size_t i = 0; i < loaded_.size(); ++i) {
            // Only what a customisation SAYS: one that is silent about the
            // control codes leaves the session's as they are.
            if (loaded_[i].linework) {
                out_.merged.linework = loaded_[i].linework;
                out_.loads[i].linework = true;
            }
            if (loaded_[i].automation) {
                out_.merged.automation = loaded_[i].automation;
                out_.loads[i].automation = true;
            }
        }
    }

    // `source` now also says `notice`: those lines first, then the lines it
    // had that are not among them, so that a notice said in both places is
    // said once.
    static void carry(CustomisationSource& source, const std::vector<std::string>& notice)
    {
        std::vector<std::string> lines = notice;
        for (std::string& line : source.notice) {
            if (std::find(notice.begin(), notice.end(), line) == notice.end()) {
                lines.push_back(std::move(line));
            }
        }
        source.notice = std::move(lines);
    }

    void mergeHeader()
    {
        const bool replace = mode_ == LoadMode::Replace;
        // The session keeps its name: a load is added to it. Unless it has
        // none yet, or the load took the place of both kinds, after which
        // none of its definitions or rules is left for the name to be about.
        const bool takesOver =
            current_.name.empty() || (replace && out_.definitionsLoaded && out_.rulesLoaded);
        const Customisation& header = takesOver ? loaded_.front() : current_;
        out_.merged.name = header.name;
        out_.merged.description = header.description;
        out_.merged.notice = header.notice;
        out_.merged.basedOn = header.basedOn;

        // A customisation that lists no sources is its own one source - the
        // session too, as Document::installCustomisation has it.
        std::vector<CustomisationSource> sources = current_.sources;
        if (sources.empty() && !current_.name.empty()) {
            sources.push_back(
                {current_.name, !current_.library.empty(), !current_.map.empty(), {}});
        }
        std::vector<CustomisationSource> brought;
        for (std::size_t i = 0; i < loaded_.size(); ++i) {
            const Customisation& load = loaded_[i];
            // Its notice goes with it as a source, except where it has just
            // become the session's own notice above.
            const bool isHeader = takesOver && i == 0;
            const std::vector<std::string> notice =
                isHeader ? std::vector<std::string>{} : load.notice;
            if (load.sources.empty()) {
                brought.push_back(
                    {load.name, !load.library.empty(), !load.map.empty(), notice});
                continue;
            }
            // It lists what went into it, and those are its sources, each
            // with its own notice. Its OWN notice is not among them: a
            // session writes it at the top level and leaves the entry of its
            // name without one (Document::customisation). Copying the list
            // and no more dropped it - an author's notice gone from every
            // customisation a session had written, the moment it was loaded
            // into another.
            const auto first = static_cast<std::ptrdiff_t>(brought.size());
            brought.insert(brought.end(), load.sources.begin(), load.sources.end());
            if (notice.empty()) {
                continue;
            }
            const auto own =
                std::find_if(brought.begin() + first, brought.end(),
                             [&](const CustomisationSource& source) {
                                 return source.name == load.name;
                             });
            if (own != brought.end()) {
                carry(*own, notice);
                continue;
            }
            // None has its name: it was written under another than the
            // session had, and nothing says which of them the notice came
            // with. Every one carries it, rather than none.
            for (auto source = brought.begin() + first; source != brought.end(); ++source) {
                carry(*source, notice);
            }
        }
        recordCustomisationLoad(sources, brought, replace && out_.definitionsLoaded,
                                replace && out_.rulesLoaded);
        out_.merged.sources = std::move(sources);
    }

    const Customisation& current_;
    std::span<const Customisation> loaded_;
    LoadMode mode_;
    CustomisationMerge out_{};
};

} // namespace

const char* toString(LoadMode mode) { return mode == LoadMode::Merge ? "merge" : "replace"; }

CustomisationMerge mergeCustomisation(const katana::entity::Customisation& current,
                                      std::span<const katana::entity::Customisation> loaded,
                                      LoadMode mode)
{
    return Merger(current, loaded, mode).run();
}

} // namespace katana::cad
