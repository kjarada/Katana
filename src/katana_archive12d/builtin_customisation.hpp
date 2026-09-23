#pragma once

// The customisation compiled INTO this build. Internal to the module.
//
// The bytes come from a generated source file (tools/embed_customisation.py,
// run by CMake), so nothing has to be found, loaded or configured at run
// time: a Katana built with a customisation simply has it.
//
// A build without one - the customisation is third-party material under its
// own licence and is not in this repository - generates an empty list, and
// Katana then draws plain lines exactly as 12d does without one.

#include <string_view>
#include <vector>

namespace katana::archive12d::detail {

struct EmbeddedFile {
    // The file's NAME, as it was in the directory it was embedded from - never
    // its path. It is what every definition read from it is stamped with
    // (LineStyle::source), so a built-in definition says which file it came
    // from exactly as a loaded one does.
    std::string_view name;
    std::string_view bytes;
};

// Each file exactly as it was read, in name order. UTF-16 or UTF-8: they go
// through decodeText like any other, so one reader serves both a built-in
// customisation and a loaded one.
[[nodiscard]] std::vector<EmbeddedFile> builtinCustomisationFiles();

} // namespace katana::archive12d::detail
