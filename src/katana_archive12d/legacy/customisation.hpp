#pragma once

// Loading the legacy customisation files: style libraries, symbol libraries
// and survey code files (mapfiles), from files (PLAN.MD 20.3, slice 6).
//
// THIS IS NOT PART OF THE PRODUCT. These readers belong to the converter
// (convert.hpp) and to their own tests, and no front end, katana_cli or
// katana_mcp links them: the program reads one format, the Katana
// customisation (entity/customisation.hpp), and a customisation in the older
// formats is converted once. The test
// the_programs_link_none_of_the_legacy_customisation_readers
// (tests/CMakeLists.txt) fails if they ever get in.
//
// THE EXTENSION DOES NOT SAY WHAT A FILE IS. `.4d` is the extension of a
// style library and, in files as they are found, of a survey code file too -
// so that extension is two different formats, and a loader that went by the
// name would read half a customisation as the wrong thing. What a file IS is
// decided by looking inside it.
//
// It needs no third-party library, so it does not cost this module the
// property that lets it build with -DKATANA_BUILD_IO=OFF.

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

    [[nodiscard]] bool empty() const { return library.empty() && map.empty(); }
    // The linestyle and symbol names the survey code files ask for that no
    // loaded library defines, in name order. A customisation need not be
    // self-contained - the plain continuous lines "0" and "1" are never
    // defined by a library - so this is reported rather than treated as a
    // fault.
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

// Loads them all, in the order given. One unreadable file fails the load and
// says which: a customisation half loaded draws the wrong thing, and that is
// worse than not drawing.
[[nodiscard]] katana::core::Result<Customisation>
readCustomisation(const std::vector<std::filesystem::path>& paths);

} // namespace katana::archive12d
