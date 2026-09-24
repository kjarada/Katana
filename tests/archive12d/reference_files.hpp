#pragma once

// The customisation the build compiles in (KATANA_CUSTOMISATION_DIR,
// resources/customisation/ by default) - the one the survey coding work was
// measured against (PLAN.MD 20.3) - for the tests that read its files.
//
// It is real third-party material under its own licence and is NOT part of
// this repository, so every test that uses it SKIPS when it is absent: the
// suite must stay green without it.
//
// Files are found BY GLOBBING the directory and asked what they are by
// looking inside them, never by name: the extension does not say what a file
// is, since `.4d` is the extension of both a style library and, as files are
// found, a survey code file. The names the built-in customisation must have
// are tested once, against builtinCustomisation (test_customisation.cpp).

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include "katana/archive12d/customisation.hpp"
#include "katana/core/text_encoding.hpp"

namespace katana::testing {

// Decoded UTF-8 text of every customisation file in the directory, split by
// what each one turned out to be, each list in file-name order so a run is
// repeatable.
struct ReferenceCustomisation {
    std::vector<std::string> libraries{};
    std::vector<std::string> mapfiles{};

    [[nodiscard]] bool present() const { return !libraries.empty() || !mapfiles.empty(); }
};

[[nodiscard]] inline ReferenceCustomisation referenceCustomisation()
{
    ReferenceCustomisation found;
    const std::filesystem::path directory{KATANA_CUSTOMISATION_FILES};
    std::error_code ignored;
    if (!std::filesystem::is_directory(directory, ignored)) {
        return found;
    }
    std::vector<std::filesystem::path> paths;
    for (const auto& entry : std::filesystem::directory_iterator(directory, ignored)) {
        if (entry.is_regular_file(ignored)) {
            paths.push_back(entry.path());
        }
    }
    std::sort(paths.begin(), paths.end());

    for (const std::filesystem::path& path : paths) {
        std::ifstream file(path, std::ios::binary);
        if (!file) {
            continue;
        }
        std::ostringstream buffer;
        buffer << file.rdbuf();
        const auto decoded = katana::core::decodeText(buffer.str());
        if (!decoded) {
            continue; // not text this reads; not a customisation file
        }
        const auto kind = katana::archive12d::customisationKind(decoded->text);
        if (!kind) {
            continue;
        }
        (*kind == katana::archive12d::CustomisationFile::MapFile ? found.mapfiles
                                                                 : found.libraries)
            .push_back(decoded->text);
    }
    return found;
}

// The paths, for the tests that load from disk rather than from text.
[[nodiscard]] inline std::vector<std::filesystem::path> referencePaths()
{
    std::vector<std::filesystem::path> paths;
    const std::filesystem::path directory{KATANA_CUSTOMISATION_FILES};
    std::error_code ignored;
    if (!std::filesystem::is_directory(directory, ignored)) {
        return paths;
    }
    for (const auto& entry : std::filesystem::directory_iterator(directory, ignored)) {
        if (entry.is_regular_file(ignored)) {
            paths.push_back(entry.path());
        }
    }
    std::sort(paths.begin(), paths.end());
    return paths;
}

} // namespace katana::testing
