#pragma once

// Loading customisations ON TOP of the one a session has.
//
// A session starts with a customisation and a person then loads their own
// beside it: a few symbols, a table of codes for one client. That load is
// MERGED into what is there, and Replace is the other choice, said in so many
// words:
//
//   Merge    A loaded definition takes the place of the one of its name. The
//            rules a load gives a key in a section take the place of that
//            key's rules in that section; everything else is kept.
//   Replace  The load takes the place of each KIND it brought: its definitions
//            become the whole library, its rules the whole survey map. A kind
//            it did not bring is kept - a file of symbols alone never empties
//            the survey codes, in either mode.
//
// What "the same rule" is, where rules have no names, is A KEY IN A SECTION. A
// customisation says one aspect of a code per section - where it goes, its
// symbol, its attributes - so a personal file that gives `WM*` a new colour
// replaces the `WM*` feature rules and leaves the `WM*` symbol alone.
// Appending instead would do nothing at all: among rules of one key the
// EARLIER wins a field (entity::SurveyMap::add), so the rules already loaded
// would outrank every rule the person had just loaded.
//
// WHERE the loaded rules go matters among rules of their own key, and in
// EVERY section, because some fields are filled by more than one: the
// string's attributes by the pipe and the attribute sections both. So every
// rule the load gives a key goes in AHEAD of every current rule of that key
// it leaves standing - at the first of them. A key the session does not have
// follows, in the order it was loaded.
//
// The rest of a customisation is merged by what it is:
//
//   colours     by name, compared as colour names are (entity::foldColourName):
//               a loaded name takes the place of the session's, in both modes.
//   linework,   taken only when the loaded customisation SAYS them. A file of
//   automation  symbols for a colleague must not reset their control codes.
//   sources     recorded (customisation_record.hpp): each loaded customisation
//               is a source, by name, with what it brought and its notice.
//               One that LISTS its sources - every customisation a session
//               writes does - brings those instead, each with its own notice;
//               its own notice, which it says at its top level, goes with
//               the source of its name, or with every source it lists when
//               none has its name (written under another name than the
//               session had, nothing says which of them the notice came
//               with). An author's notice is never dropped by a merge.
//   name,       the session's own - a load is added to it. They are the
//   description first loaded customisation's when the session has no name yet,
//   notice,     and when a Replace brought both kinds, after which nothing of
//   basedOn     the session's definitions or rules is left to be named by it.
//
// This is the merge the older style libraries and survey code files were
// loaded through, moved onto the one customisation type; the rule is the same.
// It reads no file: readCustomisationFile (customisation_host.hpp) does that.

#include <span>
#include <string>
#include <vector>

#include "katana/entity/customisation.hpp"

namespace katana::cad {

enum class LoadMode { Merge, Replace };

// "merge" or "replace".
[[nodiscard]] const char* toString(LoadMode mode);

// What one loaded customisation did to what was there before the load.
struct CustomisationLoad {
    std::string name{};
    bool definitions = false; // it brought linestyle or symbol definitions
    bool rules = false;       // it brought survey code rules
    // Definitions by name. Two loaded customisations defining one name leave
    // the later's, and so the later is the one that brought it.
    std::vector<std::string> addedDefinitions{};
    std::vector<std::string> replacedDefinitions{};
    // Rule keys, once for each section the customisation gives the key rules
    // in: one giving `WM*` three feature rules replaced one thing, not three,
    // and one giving it a feature and a symbol rule lists it twice.
    std::vector<std::string> addedKeys{};
    std::vector<std::string> replacedKeys{};
    // Colour names, as the customisation writes them.
    std::vector<std::string> addedColours{};
    std::vector<std::string> replacedColours{};
    bool linework = false;   // it says the linework codes, which were taken
    bool automation = false; // it says the automation switches, which were taken
};

struct CustomisationMerge {
    // What to install - the session untouched when there is a problem.
    katana::entity::Customisation merged{};
    // Whether the load brought any of each kind; a kind it did not bring is
    // the session's own in `merged`.
    bool definitionsLoaded = false;
    bool rulesLoaded = false;
    // One entry per loaded customisation, in load order - one that changed
    // nothing included: "loaded, and it added nothing" is an answer a person
    // wants.
    std::vector<CustomisationLoad> loads{};
    // Replace only: what the load did not bring and so is gone.
    std::vector<std::string> removedDefinitions{};
    std::vector<std::string> removedKeys{};
    // Everything that stops the load, each naming the customisation it is
    // about and then the part of it (customisationFaults,
    // customisation_state.hpp, is what is looked for). A load is ALL OR
    // NOTHING: with any problem `merged` is the session as it was, so that
    // half a load is never installed.
    std::vector<std::string> problems{};

    [[nodiscard]] bool ok() const { return problems.empty(); }
};

// `current` is the session (Document::customisation()), `loaded` the
// customisations of one load in the order they were named. Install
// `merged` with Document::installCustomisation when the result is ok() - and
// heed what that returns: the loads were judged here, the session's own
// definitions and sources are judged there.
[[nodiscard]] CustomisationMerge mergeCustomisation(
    const katana::entity::Customisation& current,
    std::span<const katana::entity::Customisation> loaded, LoadMode mode);

} // namespace katana::cad
