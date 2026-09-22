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
// for why they differ.
[[nodiscard]] katana::core::Result<Customisation>
readCustomisationInto(Customisation into, const std::filesystem::path& path);

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

} // namespace katana::archive12d
