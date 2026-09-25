#pragma once

// IMPORT and EXPORT of a .dxf, natively (katana_dxf, docs/dxf.md). Here and
// not in main.cpp so that the command line's one hook is a line, and
// here rather than behind the interoperability guard because reading a DXF
// needs no third-party library: a build without GDAL still has it.

#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"

namespace katana::app {

// `verb` is IMPORT or EXPORT, upper case; `path` the file, without
// surrounding quotes, and `local` whether an IMPORT ended in LOCAL - both as
// CommandInterpreter::importArgument reads the line. nullopt when the path is
// not a .dxf, so the caller carries on to its other importers; otherwise
// whether it worked, which has been reported on stdout or stderr.
[[nodiscard]] std::optional<bool> runDxfVerb(katana::cad::Document& document,
                                             std::string_view verb, const std::string& path,
                                             bool local);

} // namespace katana::app
