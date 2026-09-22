#pragma once

// Loading a 12d customisation: the linestyle library, the symbol library and
// the mapfile, from files (PLAN.MD 20.3, slice 6).
//
// THE EXTENSION DOES NOT SAY WHAT A FILE IS. Of the four files this was built
// against, `TfNSW_Survey_Detail.mapfile` and `names.4d` are both mapfiles
// while `user_linestyl_TfNSWv15.4d` and `user_symbols_TfNSWv15.4d` are both
// style libraries - so `.4d` is two different formats and a loader that went
// by the name would read half the customisation as the wrong thing. What a
// file IS is decided by looking inside it.
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
    // self-contained - the Transport for NSW mapfile names two symbols from
    // 12d's own standard library and the plain lines "0" and "1" - so this is
    // reported rather than treated as a fault.
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

// Loads them all, in the order given. One unreadable file fails the load and
// says which: a customisation half loaded draws the wrong thing, and that is
// worse than not drawing.
[[nodiscard]] katana::core::Result<Customisation>
readCustomisation(const std::vector<std::filesystem::path>& paths);

} // namespace katana::archive12d
