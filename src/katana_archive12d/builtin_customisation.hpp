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

// Each file exactly as it was read, in name order. UTF-16 or UTF-8: they go
// through decodeText like any other, so one reader serves both a built-in
// customisation and a loaded one.
[[nodiscard]] std::vector<std::string_view> builtinCustomisationFiles();

} // namespace katana::archive12d::detail
