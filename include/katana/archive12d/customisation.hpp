#pragma once

// Loading a 12d customisation: the linestyle library, the symbol library and
// the mapfile, from files (PLAN.MD 20.3, slice 6).
//
// THE EXTENSION DOES NOT SAY WHAT A FILE IS. Of the four files in the
// reference customisation this was built against, two are mapfiles and two
// are style libraries, and THREE of them end in `.4d` - so that extension is
// two different formats, and a loader that went by the name would read half
// the customisation as the wrong thing. What a file IS is decided by looking
// inside it.
//
// This lives here, beside the readers, rather than in a front end, because
// both front ends need it and neither should own it: `katana_cli` and the Qt
// application offer the same verb. It needs no third-party library, so it
// does not cost this module the property that lets it build with
// -DKATANA_BUILD_IO=OFF.

#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::archive12d {

enum class CustomisationFile { StyleLibrary, MapFile };

[[nodiscard]] const char* toString(CustomisationFile kind);

struct LoadedFile {
    std::filesystem::path path{};
    CustomisationFile kind = CustomisationFile::StyleLibrary;
    // Definitions or rules this file contributed, and how many of them
    // replaced one already loaded.
    std::size_t read = 0;
    std::size_t replaced = 0;
};

struct Customisation {
    katana::entity::StyleLibrary library{};
    katana::entity::SurveyMap map{};
    std::vector<LoadedFile> files{};
    // Every warning from every file, each prefixed with the file it came
    // from: one list, because a person loading four files wants one report.
    std::vector<std::string> warnings{};
    // Files that could not be read AT ALL, each prefixed with the file, and
    // each costing only itself: what was loaded before and after it is kept.
    // Only readEachCustomisationFile fills this - and so builtinCustomisation,
    // which has no caller it could fail to - because readCustomisation fails
    // whole instead, for a person who can be told and can fix the file.
    std::vector<std::string> errors{};

    [[nodiscard]] bool empty() const { return library.empty() && map.empty(); }
    // The linestyle and symbol names the mapfile asks for that no loaded
    // library defines, in name order. A customisation need not be
    // self-contained - the reference mapfile names two symbols from 12d's own
    // standard library and the plain lines "0" and "1" - so this is reported
    // rather than treated as a fault.
    [[nodiscard]] std::vector<std::string> unresolvedStyles() const;
};

// What this file holds, by looking inside it. Fails when the bytes are not
// text this can decode, or are neither a mapfile nor a style library.
[[nodiscard]] katana::core::Result<CustomisationFile> customisationKind(std::string_view text);

// Loads one file into a customisation. Later libraries win a definition,
// earlier mapfiles win a field - see entity::addOrReplace and SurveyMap::add
// for why they differ. Every definition read is stamped with the file's NAME
// (LineStyle::source), never its path.
[[nodiscard]] katana::core::Result<Customisation>
readCustomisationInto(Customisation into, const std::filesystem::path& path);

// The same for a file already in memory: `name` is what it is reported and
// stamped as, and `bytes` are its contents in any encoding decodeText reads.
[[nodiscard]] katana::core::Result<Customisation>
readCustomisationBytes(Customisation into, std::string_view name, std::string_view bytes);

struct CustomisationBytes {
    std::string name{};
    std::string_view bytes{};
};

// Loads every file, in order, where one that cannot be read costs ONLY
// ITSELF: it is named in `errors`, and what the files before and after it
// brought is kept. That is what the customisation compiled into the build
// needs - there is nobody to fail to, and an empty library because the
// fourth file was damaged would draw every drawing wrong without a word
// (audit A12-06). A person loading files uses readCustomisation instead, which
// fails whole so they can be told.
[[nodiscard]] Customisation readEachCustomisationFile(const std::vector<CustomisationBytes>& files);

// THE CUSTOMISATION COMPILED INTO THIS BUILD.
//
// Its linestyles, symbols and survey codes are part of the program: nothing
// is found, loaded or configured, and every drawing has them from the moment
// it is opened. Parsed once on first use and kept, so a session that never
// draws a survey never pays for it.
//
// Empty when this build was made without one - it is third-party material
// under its own licence and is not in this repository - and Katana then
// draws plain lines, exactly as 12d does without a customisation.
//
// A file of it that cannot be read is named in `errors` and its warnings are
// in `warnings`, for a front end to log: they are never dropped (A12-06).
[[nodiscard]] const Customisation& builtinCustomisation();

// Where a customisation is looked for when nobody names one, in order:
//
//   1. <executable>/../share/katana/customisation  - what a built application
//      ships with, so an installed Katana has its customisation without being
//      told where it is;
//   2. <executable>/../../../docs/12d Refrence Files - the same thing in a
//      development tree, where the application runs from build/<config>/bin.
//
// A customisation is site-wide data, not something to compile in: it is
// updated by the people who maintain it, on their own schedule, and a build
// of Katana should not have to be reissued because a linestyle changed. The
// files are also third-party material under their own licence, which is why
// they are not in this repository - see docs/survey_coding.md.
[[nodiscard]] std::vector<std::filesystem::path>
customisationSearchPath(const std::filesystem::path& executable);

// Every customisation file in the FIRST directory of the search path that
// holds any, in name order. Empty when there is none to find, which is not an
// error: a Katana with no customisation draws plain lines, as 12d does.
[[nodiscard]] std::vector<std::filesystem::path>
findCustomisation(const std::filesystem::path& executable);

// Loads them all, in the order given. One unreadable file fails the load and
// says which: a customisation half loaded draws the wrong thing, and that is
// worse than not drawing.
[[nodiscard]] katana::core::Result<Customisation>
readCustomisation(const std::vector<std::filesystem::path>& paths);

// ---- loading into what is already loaded ------------------------------------------------
//
// A customisation is loaded ON TOP of one: Katana starts with the one compiled
// into it, and a person loading their own symbol file wants their symbols
// ADDED, not the other 792 definitions and 1,624 survey rules thrown away
// (audit QT-21). So a load MERGES by default, and replacing is something asked
// for. Either way, a load that brought no definitions leaves the library as it
// was, and one that brought no rules leaves the map: an empty table is never
// installed by a load that did not bring one.

enum class LoadMode {
    // What the load brings takes precedence, and nothing else is lost: a
    // definition replaces the current one of the same name, and the rules the
    // load gives a key in a section replace the current rules of that key in
    // that section. Every other current definition and rule is kept - and the
    // loaded rules of a key go AHEAD of the current rules of that key that
    // are kept, so a field two sections fill (a comment; an attribute of one
    // name from pipe_data and string_attribute_data, or from vertex_pipe_data
    // and vertex_attribute_data) takes the loaded value.
    Merge,
    // What the load brings is ALL there is, of each kind it brought.
    Replace,
};

[[nodiscard]] const char* toString(LoadMode mode);

// What one file of a load did to what was loaded before it.
struct FileMerge {
    std::string name{}; // the file's name; empty for content no file claims
    CustomisationFile kind = CustomisationFile::StyleLibrary;
    // Definitions by name, or rules by key - a key once for each section the
    // file gives rules for it in - that were not in the current customisation.
    std::vector<std::string> added{};
    // Those that were, and that the file's now take the place of.
    std::vector<std::string> replaced{};
};

struct CustomisationMerge {
    // What to install. Each is the CURRENT one, unchanged, when the load
    // brought nothing of that kind - so a caller may install both
    // unconditionally - and `libraryLoaded` / `mapLoaded` say whether it did.
    katana::entity::StyleLibrary library{};
    katana::entity::SurveyMap map{};
    bool libraryLoaded = false;
    bool mapLoaded = false;
    std::vector<FileMerge> files{};
    // Replace only: current definitions and rule keys the result no longer
    // has, so a person replacing a library is told what went with it.
    std::vector<std::string> removedDefinitions{};
    std::vector<std::string> removedKeys{};
    // Definitions or rules that could not be installed. None can be today -
    // each came from a table that validated it on the way in - so this is
    // empty; it is here so that if that ever stops being true the loss is
    // listed rather than silent.
    std::vector<std::string> problems{};
};

// Merges or replaces. It returns a result rather than failing, because each
// input is already a valid library and map (see `problems`).
//
// Attribution to files: a definition belongs to the file its
// LineStyle::source names, and rules to the mapfile whose share of
// `loaded.map` they are (readCustomisationInto appends each file's rules in
// order and records how many in LoadedFile::read).
[[nodiscard]] CustomisationMerge mergeCustomisation(const katana::entity::StyleLibrary& currentLibrary,
                                                    const katana::entity::SurveyMap& currentMap,
                                                    const Customisation& loaded, LoadMode mode);

} // namespace katana::archive12d
