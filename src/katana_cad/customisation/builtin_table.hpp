#pragma once

// The Katana customisation files this build compiled into the program: what
// src/katana_cad/CMakeLists.txt embeds with #embed into one generated
// translation unit (builtin_customisation_table.cpp, in the build tree) and
// customisation/builtin_customisation.cpp reads. Internal to katana_cad - the
// public view of them is cad::builtIns() (customisation_host.hpp).

#include <span>
#include <string_view>

namespace katana::cad::detail {

struct CompiledInFile {
    // The file's name without its directory, which is what an entry of
    // cad::builtIns() gives as its `source`.
    std::string_view file;
    // Its bytes, as they were on disk when the program was built. Never
    // empty: an empty file stands for itself as one zero byte, which the
    // reader refuses in so many words.
    std::string_view bytes;
};

// Every file the build embedded, in the order it was given: the DEFAULT
// built-in first when compiledInHasDefault(), then the alternates. Empty in a
// build that names no file.
[[nodiscard]] std::span<const CompiledInFile> compiledInFiles();

// Whether the first of them is the default built-in (KATANA_BUILTIN_CUSTOMISATION
// named a file). False when only alternates exist, and then a program has no
// built-in of its own - an alternate is offered by name and never started with.
[[nodiscard]] bool compiledInHasDefault();

} // namespace katana::cad::detail
