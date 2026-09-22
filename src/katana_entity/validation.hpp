#pragma once

// What every named record in the entity layer checks about its name.
// Internal to the module.
//
// This was a file-local helper in tables.cpp serving six tables, which meant
// the seventh - the style library - would have been a second copy of it.
// CLAUDE.md section 6: a second way of doing something that already has a
// first way is a defect, not a shortcut.

#include <string_view>

#include "katana/core/error.hpp"

namespace katana::entity::detail {

// A name must be there and must be text. The UTF-8 check is here rather than
// left to each caller because every name is written into a project file and
// read back, and serialization requires valid UTF-8 of every string in the
// model - a name that is not would be rejected later, further from the
// mistake, or not at all.
[[nodiscard]] katana::core::Status validateName(std::string_view name, const char* what);

} // namespace katana::entity::detail
